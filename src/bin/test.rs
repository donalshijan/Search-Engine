use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::io::{BufRead, BufWriter};
use tokio::net::TcpStream;
use std::io::Write;
use std::fs::{File, OpenOptions};
use std::io::BufReader;
use std::collections::VecDeque;
use serde_json::Value;
use indicatif::{ProgressBar, ProgressStyle};

async  fn test_query_performance() -> Result<(), Box<dyn std::error::Error>> {
    // Open the queries.txt file
    let file = File::open("queries.txt")?;
    let reader = BufReader::new(file);
    
    let mut query_ids = VecDeque::new(); // To store query IDs
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
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} requests sent ({eta})")?
            .progress_chars("#>-"),
    );
    
    response_progress.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.magenta/blue}] {pos}/{len} responses received ({eta})")?
            .progress_chars("#>-"),
    );

    // Open the test.log file for logging
    let log_file = OpenOptions::new()
        .append(true)  // Open in append mode
        .create(true)  // Create the file if it doesn't exist
        .open("test.log")?;
    let mut log_writer = BufWriter::new(log_file);

    // Loop to send each query
    for query in lines {
        
        // Create a TcpStream to connect to the server (sending query)
        let mut stream = TcpStream::connect("127.0.0.1:8080").await?;
        
        // Send the query directly
        stream.write_all(query.as_bytes()).await?;
        
        // Read the response
        let mut buffer = [0u8; 1024];
        let number_of_bytes_read = stream.read(&mut buffer).await?;
        let response_json = &buffer[..number_of_bytes_read];
        
        // Deserialize the response to extract `query_id`
        let response_value: Value = serde_json::from_slice(response_json)?;
        if let Some(query_id) = response_value["query_id"].as_str() {
            query_ids.push_back(query_id.to_string());
        }

        query_count += 1;
        request_progress.inc(1); // Increment request progress bar
    }
    request_progress.finish_with_message("All requests sent.");
    
    // Loop to get the results for each query
    while let Some(query_id) = query_ids.pop_front() {
        // Create a TcpStream to connect to the results server (fetching result)
        let mut stream = TcpStream::connect("127.0.0.1:8081").await?;
        
        // Create a request with the query_id and send it
        let request = serde_json::json!({ "query_id": query_id });
        stream.write_all(request.to_string().as_bytes()).await?;
        
        // Read the response
        let mut buffer = [0u8; 1024];
        let number_of_bytes_read = stream.read(&mut buffer).await?;
        let response_json = &buffer[..number_of_bytes_read];
        
        // Deserialize the response to extract `query_processing_time`
        let response_value: Value = serde_json::from_slice(response_json)?;
        if let Some(processing_time) = response_value["query_processing_time"].as_f64() {
            total_processing_time += processing_time;
            // Log the `query_id` and `query_processing_time` to test.log
            writeln!(
                log_writer,
                "Query ID: {}, Processing Time: {:.8} seconds",
                query_id, processing_time
            )?;
            log_writer.flush()?;
        }
        response_progress.inc(1); // Increment response progress bar
    }
    response_progress.finish_with_message("All responses received.");
    // Calculate the average processing time
    let average_processing_time = total_processing_time / query_count as f64;
    
    // Write the result to result.txt
    let mut result_file = OpenOptions::new()
    .append(true)  // Open in append mode
    .create(true)  // Create the file if it doesn't exist
    .open("result.txt")?;
    writeln!(result_file, "Average query processing time: {}", average_processing_time)?;

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