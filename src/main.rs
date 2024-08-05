use search_engine::*;
use std::io::{self, Write};
use std::thread;
use num_cpus;
use core_affinity;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

enum EngineMode {
    SingleCoreSingleThread,
    MultiCoreEachThreadSearchingAgainstWholeIndex,
    MultiCoreEachThreadInSameCoreSearchingAgainstSingleShardedSubsetOfIndex
}

struct ThreadData {
    id: usize,
    handle: thread::JoinHandle<()>,
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
    pub fn start_engine(&self,documents: Vec<Document>) {
        match self.engine_mode {
            EngineMode::SingleCoreSingleThread => {
                // Handle SingleInstanceSingleCore
                let mut searcher = Searcher::new();
                for doc in &documents {
                    searcher.add_document_to_index(doc);
                }
                process_queries(searcher);
            }
            EngineMode::MultiCoreEachThreadSearchingAgainstWholeIndex => {
                // Handle SingleInstanceEachCoreNoSharding
                let num_cores = core_affinity::get_core_ids().unwrap().len();
                let total_threads = num_cpus::get();
                let num_threads_per_core = if num_cores > 0 {
                    total_threads / num_cores
                } else {
                    1 // Default to 1 if no cores are found
                };
                let mut searcher =Searcher::new();
                let cores = core_affinity::get_core_ids().unwrap();
                let docs_clone = documents.clone();
                for doc in docs_clone {
                    searcher.add_document_to_index(&doc);
                }
                let searcher = Arc::new(Mutex::new(searcher));
                let mut handles = Vec::new();
                // Spawn a thread for each core, and set its affinity
                for (i,core_id) in cores.into_iter().enumerate() {
                    for j in 0..num_threads_per_core {
                        let searcher_clone = Arc::clone(&searcher);
                        let thread_id = i * num_threads_per_core + j;
                        let handle = thread::spawn(move || {
                            // Pin this thread to the specific core
                            core_affinity::set_for_current(core_id);
                            process_queries(searcher_clone);
                        });
            
                        handles.push(ThreadData { id: thread_id, handle });
                    }
                }
                // Join all threads
                for thread_data in handles {
                    match thread_data.handle.join() {
                        Ok(_) => println!("Thread ID {} completed successfully.", thread_data.id),
                        Err(e) => eprintln!("Thread ID {} panicked: {:?}", thread_data.id, e),
                    }
                }
            }
            EngineMode::MultiCoreEachThreadInSameCoreSearchingAgainstSingleShardedSubsetOfIndex => {
                // Handle SingleInstanceEachCoreSharded
                let num_cores = core_affinity::get_core_ids().unwrap().len();
                let total_threads = num_cpus::get();
                let num_threads_per_core = if num_cores > 0 {
                    total_threads / num_cores
                } else {
                    1 // Default to 1 if no cores are found
                };
                let shard_size = documents.len() / num_cores;
                let mut handles = Vec::new();
                let cores = core_affinity::get_core_ids().unwrap();
                for (i,core_id) in cores.into_iter().enumerate() {
                    let mut searcher = Searcher::new();
                    let shard = &documents[i * shard_size..(i + 1) * shard_size];
                    for doc in shard {
                        searcher.add_document_to_index(doc);
                    }
                    let searcher = Arc::new(Mutex::new(searcher));
                    for j in 0..num_threads_per_core {
                        let thread_id = i * num_threads_per_core + j;
                        let searcher_clone = Arc::clone(&searcher);
                        let handle = thread::spawn(move || {
                            // Pin this thread to the specific core
                            core_affinity::set_for_current(core_id);
                            process_queries(searcher_clone);
                        });
            
                        handles.push(ThreadData { id: thread_id, handle });
                    }
                }
                // Join all threads
                for thread_data in handles {
                    match thread_data.handle.join() {
                        Ok(_) => println!("Thread ID {} completed successfully.", thread_data.id),
                        Err(e) => eprintln!("Thread ID {} panicked: {:?}", thread_data.id, e),
                    }
                }
            }
        }
    }
}

struct QueryQueue {
    queue: Arc<Mutex<VecDeque<Query>>>,
}

impl QueryQueue {
    // Create a new empty queue
    fn new() -> Self {
        QueryQueue {
            queue: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    // Add a new query to the queue
    fn enqueue(&self, query: Query) {
        let mut queue = self.queue.lock().unwrap();
        queue.push_back(query);
    }

    // Remove and return a query from the queue
    fn dequeue(&self) -> Option<Query> {
        let mut queue = self.queue.lock().unwrap();
        queue.pop_front()
    }

    // Check the current size of the queue
    fn size(&self) -> usize {
        let queue = self.queue.lock().unwrap();
        queue.len()
    }
}

// Function to listen for user queries and add them to the queue
fn listen_for_user_queries(queue: Arc<QueryQueue>) {
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
        let query = Query::new(
                    &unique_id,
                    &input,
                );

        // Enqueue the query
        queue.enqueue(query);
    }
}

fn main() {
    // Example documents
    let doc1 = Document::new("doc1", DocumentFormat::PlainText("Rust is a systems programming language.".into()));
    let doc2 = Document::new("doc2", DocumentFormat::PlainText("Search engines are essential for the web.".into()));
    let documents = vec![doc1, doc2];

    // Create a new query queue
    let query_queue = Arc::new(QueryQueue::new());

    // Run the listen_for_user_queries function on a separate thread
    let queue_clone = Arc::clone(&query_queue);
    thread::spawn(move || {
        listen_for_user_queries(queue_clone);
    });

    let mode = EngineMode::SingleCoreSingleThread; // Set the desired mode here
    let engine= Engine::new(mode);
    engine.start_engine(documents);
    // The main thread can do other tasks, or join the spawned thread if necessary
    loop {
        // Here you might process queries, or just keep the main thread alive
        thread::sleep(std::time::Duration::from_secs(1));
        }
}
