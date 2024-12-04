use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;
use serde::{Serialize, Deserialize};
use crossbeam::channel;
use search_engine::*;

#[derive(Serialize, Deserialize)]
struct QueryRequest {
    query: String,
}

#[derive(Serialize)]
struct QueryRequestResponse {
    query_id: String,
    message: String,
}

#[derive(Serialize, Deserialize)]
struct QueryResultRequest {
    query_id: String,
}

#[derive(Serialize)]
struct QueryResultResponse {
    message: String,
    query_processing_time: Duration,
}


pub async fn run_query_requests_server(query_tx:channel::Sender<QueryChannelSenderMessage>) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    loop {
        let (mut socket, _) = listener.accept().await?;
        let query_tx_clone = query_tx.clone();
        tokio::spawn(async move {
            let mut buffer = [0u8; 1024];

            let n = socket.read(&mut buffer).await.unwrap();
            let request: QueryRequest = serde_json::from_slice(&buffer[..n]).unwrap();

            let unique_id = Uuid::new_v4().to_string();
            let query = Query::new(&unique_id, &request.query);

            query_tx_clone.send(QueryChannelSenderMessage::Query(query)).unwrap();

            // Create the response
            let response = QueryRequestResponse {
                query_id: unique_id.clone(),
                message: format!("Query received: {}", request.query),
            };

            // Serialize the response to JSON
            let response_json = serde_json::to_string(&response).unwrap();
            socket.write_all(response_json.as_bytes()).await.unwrap();
        });
    }
}

pub async fn run_get_results_server(query_results: Arc<QueryResults>) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:8081").await?;
    
    loop {
        let (mut socket, _) = listener.accept().await?;
        let query_results_clone = Arc::clone(&query_results);

        tokio::spawn(async move {
            let mut buffer = [0u8; 1024];

            // Read query ID request from client
            let n = socket.read(&mut buffer).await.unwrap();
            let request: QueryResultRequest = serde_json::from_slice(&buffer[..n]).unwrap();

            // Get the query result
            let result: Option<QueryResult> = query_results_clone.get_query_result(&request.query_id);
            let result_clone: Option<QueryResult> = result.clone();
            let response_message: String = match result_clone {
                Some(result_clone) => {
                    format!("Query result: {}", result_clone)
                }
                None => {
                    String::from("No result yet, check again...")
                }
            };
            
            let query_processing_time = match result{
                Some(result)=>{
                    result.query_processing_time
                }
                None =>{
                    Duration::ZERO
                }
            };
            // Create the response
            let response = QueryResultResponse {
                message: response_message,
                query_processing_time: query_processing_time,
            };

            // Serialize the response to JSON
            let response_json = serde_json::to_string(&response).unwrap();
            socket.write_all(response_json.as_bytes()).await.unwrap();
        });
    }
}