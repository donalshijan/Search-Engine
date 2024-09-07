use search_engine::test_query_performance;

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