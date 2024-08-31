use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use serde::{Serialize, Deserialize};
use std::time::Duration;
use tokio::time::sleep;
use search_engine::*;

#[derive(Serialize, Deserialize)]
struct QueryRequest {
    query: String,
}

#[derive(Serialize)]
struct QueryResponse {
    query_id: String,
    message: String,
}


pub async fn run_server(query_queue: Arc<QueryQueue>,query_results: Arc<QueryResults>) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;

    loop {
        let (mut socket, _) = listener.accept().await?;
        let queue_clone = Arc::clone(&query_queue);
        let query_results_clone = Arc::clone(&query_results);
        tokio::spawn(async move {
            let mut buffer = [0u8; 1024];

            let n = socket.read(&mut buffer).await.unwrap();
            let request: QueryRequest = serde_json::from_slice(&buffer[..n]).unwrap();

            let unique_id = Uuid::new_v4().to_string();
            let query = Query::new(&unique_id, &request.query);

            queue_clone.enqueue(query);

            // Create the response
            let response = QueryResponse {
                query_id: unique_id.clone(),
                message: format!("Query received: {}", request.query),
            };

            // Serialize the response to JSON
            let response_json = serde_json::to_string(&response).unwrap();
            socket.write_all(response_json.as_bytes()).await.unwrap();
             // Poll for the result
            loop {
                let result = query_results_clone.get_query_result(&unique_id);
                match result {
                    Some(result) => {
                        println!("Query result: {}", result);
                        break; // Exit the polling loop if result is obtained
                    }
                    None => {
                        println!("No result yet, checking again...");
                        // Wait for a short period before polling again
                        sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });
    }
}
