use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::io::{BufRead, BufWriter};
use tokio::net::TcpStream;
use tokio::time::{sleep, Duration};
use std::io::Write;
use std::fs::{File, OpenOptions};
use std::io::BufReader;
use std::collections::{HashMap, VecDeque};
use serde_json::Value;
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct QueryRequest {
    query: String,
}

async  fn test_query_performance() -> Result<(), Box<dyn std::error::Error>> {
    // Open the queries.txt file
    let file = File::open("queries.txt")?;
    let reader = BufReader::new(file);
    
    let mut query_ids = VecDeque::new(); // To store query IDs
    let mut queries: HashMap<String,i32> = HashMap::new(); // To store query_id -> query_no mapping
    let mut total_accuracy = 0.0;   // To store the cumulative accuracy
    let mut last_query_no = 0;      // Keep track of the previous query number
    let mut total_processing_time = 0.0;
    let mut query_count = 0;

    // Read all lines to determine total queries for the progress bar
    let lines: Vec<String> = reader.lines().collect::<Result<_, _>>()?;
    let total_queries = lines.len();
    
    // Initialize progress bars
    let request_progress = ProgressBar::new(total_queries as u64);
    let response_progress = ProgressBar::new(total_queries as u64);

    request_progress.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} requests sent")?
            .progress_chars("#>-"),
    );
    
    response_progress.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.magenta/blue}] {pos}/{len} responses received")?
            .progress_chars("#>-"),
    );
    // Open the test.log file for logging
    let log_file = OpenOptions::new()
        .append(true)  // Open in append mode
        .create(true)  // Create the file if it doesn't exist
        .open("test.log")?;
    let mut log_writer = BufWriter::new(log_file);
    println!("Making query requests");
    // Loop to send each query
    for query in lines {
        // Create a TcpStream to connect to the server (sending query)
        let mut stream = TcpStream::connect("127.0.0.1:8080").await?;
        // Send the query directly
        let query_json = serde_json::to_string(&QueryRequest { query }).unwrap();
        stream.write_all(query_json.as_bytes()).await?;
        // Read the response
        let mut buffer = [0u8; 1024];
        let number_of_bytes_read = stream.read(&mut buffer).await?;
        let response_json = &buffer[..number_of_bytes_read];
        // Deserialize the response to extract `query_id`
        let response_value: Value = serde_json::from_slice(response_json)?;
        if let Some(query_id) = response_value["query_id"].as_str() {
            let query_no = if (last_query_no + 1) % 10 == 0 {
                10
            } else {
                (last_query_no + 1) % 10
            };
            last_query_no = query_no;

            queries.insert(query_id.to_string(), query_no);
            query_ids.push_back(query_id.to_string());
        }
        query_count += 1;
        request_progress.inc(1); // Increment request progress bar
    }
    request_progress.finish_with_message("All requests sent.");
    println!("Getting responses");
    // Loop to get the results for each query
    while let Some(query_id) = query_ids.pop_front() {
        
        loop{
            // Create a TcpStream to connect to the results server (fetching result)
            let mut stream = TcpStream::connect("127.0.0.1:8081").await?;
            // Create a request with the query_id and send it
            let request = serde_json::json!({ "query_id": query_id });
            stream.write_all(request.to_string().as_bytes()).await?;
            
            // Read the response in chunks until a complete JSON is received
            let mut response_data = Vec::new();
            let mut buffer = [0u8; 1024];

            loop {
                let n = stream.read(&mut buffer).await?;
                if n == 0 {
                    // End of stream
                    break;
                }
                response_data.extend_from_slice(&buffer[..n]);

                // Check if the buffer contains a valid JSON (break out when complete)
                if let Ok(_) = serde_json::from_slice::<Value>(&response_data) {
                    break;
                }
            }
            // Deserialize the response to extract `message` and `query_processing_time`
            let response_value: Value = serde_json::from_slice(&response_data)?;
            // println!("Response:{}",response_value);
            if let Some(message) = response_value["message"].as_str() {
                if message == "No result yet, check again..." {
                    // Wait briefly before retrying
                    sleep(Duration::from_millis(100)).await;
                    continue;
                }
            }
            if let Some(documents) = response_value["documents"].as_array() {
                // Extract query_no from index
                if let Some(&query_no) = queries.get(&query_id) {
                    let expected_prefix = format!("query{}", query_no);
                    let mut matching_count = 0;

                    for doc in documents.iter().take(10) {
                        if let Some(document_id) = doc[0].as_str() {
                            if document_id.starts_with(&expected_prefix) {
                                matching_count += 1;
                            }
                        }
                    }

                    let accuracy = (matching_count as f64 / 10.0) * 100.0;
                    total_accuracy += accuracy;
                    let mut processing_time : f64 = 0.0;
                    if let Some(processing_time_obj) = response_value["query_processing_time"].as_object() {
                        if let Some(secs) = processing_time_obj.get("secs").and_then(|v| v.as_u64()) {
                            if let Some(nanos) = processing_time_obj.get("nanos").and_then(|v| v.as_u64()) {
                                processing_time = secs as f64 + (nanos as f64) / 1_000_000_000.0;
                                total_processing_time += processing_time;
                            }
                        }
                    }

                    // Log processing time and accuracy for this query 
                    writeln!(
                        log_writer,
                        "Query ID: {}, Processing Time: {:.8} seconds, Accuracy: {:.2}%",
                        query_id, processing_time, accuracy
                    )?;
                    log_writer.flush()?;
                }
            }
            break;
        }
        response_progress.inc(1); // Increment response progress bar
    }
    response_progress.finish_with_message("All responses received.");
    // Calculate the average processing time
    let average_processing_time = total_processing_time / query_count as f64;
    // Calculate the average accuracy
    let average_accuracy = total_accuracy / query_count as f64;
    
    // Write the result to results.txt
    let mut result_file = OpenOptions::new()
    .append(true)  // Open in append mode
    .create(true)  // Create the file if it doesn't exist
    .open("results.txt")?;
    writeln!(result_file, "Average query processing time: {} secs", average_processing_time)?;
    writeln!(result_file, "Average accuracy: {:.2}%", average_accuracy)?;

    Ok(())
}

#[tokio::main]
async fn main() {
    match test_query_performance().await {
        Ok(_) => {
            println!("Test finished successfully.");
        }
        Err(e) => {
            eprintln!("Test failed: {}", e);
        }
    }
}