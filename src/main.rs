mod server;
mod processor;

use processor::Processor;
use search_engine::*;
use std::io::{self, Write};
use std::thread::{self, ThreadId};
use num_cpus;
use core_affinity;
use std::sync::{Arc, RwLock};
use uuid::Uuid;
use crossbeam::channel;



enum EngineMode {
    SingleCoreSingleThread,
    MultiCoreMultipleThreadsEachThreadSearchingAgainstWholeIndex,
    MultiCoreMultipleThreadsEachThreadInSameCoreSearchingAgainstSingleShardedSubsetOfIndex
}

struct ThreadData {
    processor_id: usize,
    handle: thread::JoinHandle<ThreadId>,
}


struct Engine {
    engine_mode: EngineMode,
}

impl Engine {
    // Constructor
    pub fn new(engine_mode: EngineMode) -> Self {
        Engine { engine_mode }
    }

    // Start the engine based on the engine mode
    pub fn start_engine(&self,documents: Vec<Document>,query_rx: channel::Receiver<Query> ,query_results: Arc<QueryResults>) {
        match self.engine_mode {
            EngineMode::SingleCoreSingleThread => {
                // Handle SingleInstanceSingleCore
                let mut search_library = SearchLibrary::new();
                for doc in &documents {
                    search_library.add_document_to_index(doc);
                }
                let search_library = Arc::new(RwLock::new(search_library));
                let cores = core_affinity::get_core_ids().unwrap();

                let handle = thread::spawn(move || {
                    // Pin this thread to the specific core
                    core_affinity::set_for_current(cores[0]);
                    let thread_id = thread::current().id();
                    let processor = Processor::new(1,search_library,Arc::new(RwLock::new(vec![cores[0]])),Arc::new(RwLock::new(vec![thread_id])),query_rx,query_results);
                    let processor=processor.store_in_global();
                    processor.process_queries();
                    thread_id
                });  
                match handle.join() {
                    Ok(_) => println!("Processor Terminated."),
                    Err(e) => eprintln!("Processor thread panicked: {:?}", e),
                }
            }
            EngineMode::MultiCoreMultipleThreadsEachThreadSearchingAgainstWholeIndex => {
                // Handle SingleInstanceEachCoreNoSharding
                let num_cores = core_affinity::get_core_ids().unwrap().len();
                let total_threads = num_cpus::get();
                let num_threads_per_core = if num_cores > 0 {
                    total_threads / num_cores
                } else {
                    1 // Default to 1 if no cores are found
                };
                let mut search_library =SearchLibrary::new();
                let cores = core_affinity::get_core_ids().unwrap();
                let docs_clone = documents.clone();
                for doc in docs_clone {
                    search_library.add_document_to_index(&doc);
                }
                let search_library = Arc::new(RwLock::new(search_library));
                let mut handles: Vec<ThreadData> = Vec::new();
                // Spawn a thread for each core, and set its affinity
                for (i,core_id) in cores.into_iter().enumerate() {
                    for j in 0..num_threads_per_core {
                        let processor_id = i * num_threads_per_core + j+1;
                        // let query_queue_clone = Arc::clone(&query_queue);
                        let query_rx_clone = query_rx.clone();
                        let query_results_clone = Arc::clone(&query_results);
                        let search_library_clone = Arc::clone(&search_library);
                        let handle = thread::spawn(move || {
                            // Pin this thread to the specific core
                            core_affinity::set_for_current(core_id);
                            let thread_id = thread::current().id();
                            let processor = Processor::new(processor_id,search_library_clone,Arc::new(RwLock::new(vec![core_id])),Arc::new(RwLock::new(vec![thread_id])),query_rx_clone,query_results_clone);
                            let processor=processor.store_in_global();
                            processor.process_queries();
                            thread_id
                        });
            
                        handles.push(ThreadData { processor_id, handle });
                    }
                }
                // Join all threads
                for thread_data in handles {
                    match thread_data.handle.join() {
                        Ok(_) => println!("Processor no. {} Terminated", thread_data.processor_id),
                        Err(e) => eprintln!("Processor no. {}'s running Thread panicked: {:?}", thread_data.processor_id, e),
                    }
                }
            }
            EngineMode::MultiCoreMultipleThreadsEachThreadInSameCoreSearchingAgainstSingleShardedSubsetOfIndex => {
                let mut handles = Vec::new();
                let cores = core_affinity::get_core_ids().unwrap();
                let mut search_library = SearchLibrary::new();
                let docs_clone = documents.clone();
                for doc in docs_clone {
                    search_library.add_document_to_index(&doc);
                }
                let search_library = Arc::new(RwLock::new(search_library));
                for (i,core_id) in cores.iter().enumerate(){
                    let processor_id = i+1;
                    let query_rx_clone = query_rx.clone();
                    let query_results_clone = Arc::clone(&query_results);
                    let search_library_clone = Arc::clone(&search_library);
                    let core_id = *core_id;
                    let handle = thread::spawn(move || {
                        // Pin this thread to the specific core
                        core_affinity::set_for_current(core_id);
                        let thread_id = thread::current().id();
                        let processor = Processor::new(processor_id,search_library_clone,Arc::new(RwLock::new(vec![core_id])),Arc::new(RwLock::new(vec![thread_id])),query_rx_clone,query_results_clone);
                        let processor=processor.store_in_global();
                        processor.process_each_query_across_all_threads_in_a_core();
                        thread_id
                    });
        
                    handles.push(ThreadData { processor_id, handle });

                }

                //********************** */
                /*
                let num_cores = core_affinity::get_core_ids().unwrap().len();
                let total_threads = num_cpus::get();
                let num_threads_per_core = if num_cores > 0 {
                    total_threads / num_cores
                } else {
                    1 // Default to 1 if no cores are found
                };
                 let shard_size = documents.len() / num_cores;
                for i in 0..num_threads_per_core {
                    let mut search_library = SearchLibrary::new();
                    let shard = &documents[i * shard_size..(i + 1) * shard_size];
                    for doc in shard {
                        search_library.add_document_to_index(doc);
                    }
                    let search_library = Arc::new(Mutex::new(search_library));
                    // let search_library_clone = Arc::clone(&search_library);
                    for (j,core_id) in cores.iter().enumerate() {
                        let thread_id = j * num_threads_per_core + i;
                        let query_queue_clone = Arc::clone(&query_queue);
                        let query_results_clone = Arc::clone(&query_results);
                        let search_library_clone = Arc::clone(&search_library);
                        let core_id = *core_id;
                        let handle = thread::spawn(move || {
                            // Pin this thread to the specific core
                            core_affinity::set_for_current(core_id);
                            let thread_id = thread::current().id();
                            let processor = Processor::new(search_library_clone,core_id,thread_id);
                            processor.process_queries(query_queue_clone,query_results_clone);
                        });
            
                        handles.push(ThreadData { id: thread_id, handle });
                    }
                }*/
                // Join all threads
                for thread_data in handles {
                    match thread_data.handle.join() {
                        Ok(_) => println!("Processor no. {} Terminated", thread_data.processor_id),
                        Err(e) => eprintln!("Processor no. {}'s running Thread panicked: {:?}", thread_data.processor_id, e),
                    }
                }
            }
        }
    }
}

// Function to listen for user queries and add them to the queue
fn listen_for_user_queries(query_tx: channel::Sender<Query>,query_results: Arc<QueryResults>) {
    loop {
        // Prompt the user
        print!("Enter your query: ");
        io::stdout().flush().unwrap(); // Ensure the prompt is displayed

        // Read user input
        let mut input = String::new();
        io::stdin().read_line(&mut input).expect("Failed to read line");
        let input = input.trim().to_string();

        if input.is_empty() {
            continue;
        }

        let unique_id = Uuid::new_v4().to_string();

        // Create a new Query object
        let query = Query::new(&unique_id, &input);

        // Enqueue the query
        query_tx.send(query).unwrap();
        
        // Poll for the result
        loop {
            let result = query_results.get_query_result(&unique_id);
            match result {
                Some(result) => {
                    println!("Query result: {}", result);
                    break; // Exit the polling loop if result is obtained
                }
                None => {
                    println!("No result yet, checking again...");
                    // Wait for a short period before polling again
                }
            }
        }
    }
}

fn main() {
    // Example documents 
    let doc1 = Document::new("doc1", DocumentFormat::PlainText("Rust is a systems programming language.".into()));
    let doc2 = Document::new("doc2", DocumentFormat::PlainText("Search engines are essential for the web.".into()));
    let documents = vec![doc1, doc2];

    // Create a new query queue
    let (query_tx, query_rx): (channel::Sender<Query>, channel::Receiver<Query>) = channel::unbounded();
    let query_rx_clone: channel::Receiver<Query> = query_rx.clone();
    let query_results = Arc::new(QueryResults::new());
    // Run the listen_for_user_queries function on a separate thread
    let query_tx_clone: channel::Sender<Query> = query_tx.clone();
    let query_results_clone = Arc::clone(&query_results);
    thread::spawn(move || {
        listen_for_user_queries(query_tx_clone,query_results_clone);
    });

     // Start the async server
     let server_query_tx_clone: channel::Sender<Query> = query_tx.clone();
     thread::spawn(move || {
         tokio::runtime::Runtime::new().unwrap().block_on(server::run_query_requests_server(server_query_tx_clone)).unwrap();
     });
     let server_query_results_clone = Arc::clone(&query_results);
     thread::spawn(move || {
         tokio::runtime::Runtime::new().unwrap().block_on(server::run_get_results_server(server_query_results_clone)).unwrap();
     });

    let mode = EngineMode::SingleCoreSingleThread; // Set the desired mode here
    let engine= Engine::new(mode);
    let query_results_clone = Arc::clone(&query_results);
    engine.start_engine(documents,query_rx_clone,query_results_clone);
    // The main thread can do other tasks, or join the spawned thread if necessary
    loop {
        // Here you might process queries, or just keep the main thread alive
        thread::sleep(std::time::Duration::from_secs(1));
        }
}
