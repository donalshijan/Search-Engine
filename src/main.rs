mod server;
mod processor;

use processor::processor::Processor;
use search_engine::*;
use std::time::Duration;
use std::{env, fs};
use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, sleep};
use num_cpus;
use core_affinity;
use std::sync::{Arc, Mutex, RwLock};
use uuid::Uuid;
use crossbeam::channel;
use nix::unistd::{fork, ForkResult};
use nix::sys::wait::waitpid;
use nix::sys::wait::WaitStatus;
use nix::sys::signal::{kill, Signal};
use std::process::exit;


use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;

use processor::processor::MONITOR;
use processor::processor::cleanup_shared_memory;
use processor::processor::initialize_shared_monitor;
use processor::processor::start_logging_monitor_info;
use processor::processor::{SHM_SIZE,SHM_PTR};


enum EngineMode {
    SingleCoreSingleThread,
    MultiCoreSingleThread,
    MultiCoreMultipleThreadsEachThreadSearchingAgainstWholeIndex,
    MultiCoreMultipleThreadsEachThreadInEachCoreSearchingAgainstSingleShardedSubsetOfIndex
}


impl EngineMode {
    fn from_str(mode: &str) -> Option<EngineMode> {
        match mode.to_lowercase().as_str() {
            "single_core_single_thread" => Some(EngineMode::SingleCoreSingleThread),
            "multi_core_single_thread" => Some(EngineMode::MultiCoreSingleThread),
            "multi_core_multiple_threads_each_thread_searching_against_whole_index" => Some(EngineMode::MultiCoreMultipleThreadsEachThreadSearchingAgainstWholeIndex),
            "multi_core_multiple_threads_each_thread_in_each_core_searching_against_single_sharded_subset_of_index" => Some(EngineMode::MultiCoreMultipleThreadsEachThreadInEachCoreSearchingAgainstSingleShardedSubsetOfIndex),
            _ => None,
        }
    }
}

struct ThreadData {
    processor_id: usize,
    // handle: thread::JoinHandle<ThreadId>,
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
    pub fn start_engine(&self,documents: Vec<Document>,query_rx: channel::Receiver<QueryChannelSenderMessage> ,query_results: Arc<QueryResults>,tx_processors_shutdown: Sender<String>) {
        match self.engine_mode {
            EngineMode::SingleCoreSingleThread => {
                // Handle SingleInstanceSingleCore
                let mut search_library = SearchLibrary::new();
                for doc in &documents {
                    search_library.add_document_to_index(doc);
                }
                println!("Search Library Constructed.");
                let search_library = Arc::new(RwLock::new(search_library));
                let cores = core_affinity::get_core_ids().unwrap();

                let handle = thread::spawn(move || {
                    // Pin this thread to the specific core
                    core_affinity::set_for_current(cores[0]);
                    let thread_id = thread::current().id();
                    let processor = Processor::new(1,search_library,Arc::new(RwLock::new(vec![cores[0]])),Arc::new(RwLock::new(vec![thread_id])),query_rx,query_results);
                    let processor=processor.add_to_monitor();
                    println!("Processor running...");
                    processor.process_queries();
                    // thread_id
                });  
                sleep(Duration::from_millis(100));
                match handle.join() {
                    Ok(_) => println!("Processor Terminated."),
                    Err(e) => eprintln!("Processor thread terminated due to a panic: {:?}", e),
                }
                // Send a message through the transmitter to notify processor shutdown status to the main thread
                if let Err(e) = tx_processors_shutdown.send("All processors have been shut down.".to_string()) {
                    eprintln!("Failed to send shutdown notification: {}", e);
                }
            }
            EngineMode::MultiCoreSingleThread => {
                // Handle SingleInstanceSingleCore
                let mut search_library = SearchLibrary::new();
                for doc in &documents {
                    search_library.add_document_to_index(doc);
                }
                println!("Search Library Constructed.");
                let search_library = Arc::new(RwLock::new(search_library));
                // Get the number of physical cores
                let physical_cores = num_cpus::get_physical();
                let core_ids = core_affinity::get_core_ids().unwrap();
                let mut handles: Vec<ThreadData> = Vec::new();
                for i in 0..physical_cores {
                    let core_id = core_ids[i];
                    let processor_id = i+1;
                    let query_rx = query_rx.clone();
                    let query_results = Arc::clone(&query_results);
                    let search_library = Arc::clone(&search_library);
                    let handle = thread::spawn(move || {
                        // Pin this thread to the specific core
                        core_affinity::set_for_current(core_id);
                        let thread_id = thread::current().id();
                        let processor = Processor::new(i,search_library,Arc::new(RwLock::new(vec![core_id])),Arc::new(RwLock::new(vec![thread_id])),query_rx,query_results);
                        let processor=processor.add_to_monitor();
                        println!("Processor no.{} running...",processor_id);
                        processor.process_queries();
                        // thread_id
                    }); 
                    handles.push(ThreadData { processor_id, handle });
                }
                sleep(Duration::from_millis(100));
                println!("All processors up and running.");
                // Join all threads
                for thread_data in handles {
                    match thread_data.handle.join() {
                        Ok(_) => println!("Processor no. {} Terminated", thread_data.processor_id),
                        Err(e) => eprintln!("Processor no. {}'s main thread terminated due to a panic: {:?}", thread_data.processor_id, e),
                    }
                }
                if let Err(e) = tx_processors_shutdown.send("All processors have been shut down.".to_string()) {
                    eprintln!("Failed to send shutdown notification: {}", e);
                }

            }
            EngineMode::MultiCoreMultipleThreadsEachThreadSearchingAgainstWholeIndex => {
                // Handle SingleInstanceEachCoreNoSharding
                // Get the number of physical cores
                let physical_cores = num_cpus::get_physical();
                let total_threads = num_cpus::get();
                let num_threads_per_core_recommmended = if physical_cores > 0 {
                    total_threads / physical_cores
                } else {
                    1 // Default to 1 if no cores are found
                };
                // If it happens to be 1 then set it to use atleast 2
                let mut num_threads_per_core=num_threads_per_core_recommmended;
                if num_threads_per_core_recommmended == 1{
                    num_threads_per_core=2;
                }
                let mut search_library =SearchLibrary::new();
                let core_ids = core_affinity::get_core_ids().unwrap();
                let docs_clone = documents.clone();
                for doc in docs_clone {
                    search_library.add_document_to_index(&doc);
                }
                println!("Search Library Constructed.");
                let search_library = Arc::new(RwLock::new(search_library));
                let mut handles: Vec<ThreadData> = Vec::new();
                // Spawn a thread for each core, and set its affinity
                for i in 0..physical_cores {
                    let core_id = core_ids[i];
                    for j in 0..num_threads_per_core {
                        let processor_id = i * num_threads_per_core + j+1;
                        // let query_queue_clone = Arc::clone(&query_queue);
                        let query_rx = query_rx.clone();
                        let query_results = Arc::clone(&query_results);
                        let search_library = Arc::clone(&search_library);
                        let handle = thread::spawn(move || {
                            // Pin this thread to the specific core
                            core_affinity::set_for_current(core_id);
                            let thread_id = thread::current().id();
                            let processor = Processor::new(processor_id,search_library,Arc::new(RwLock::new(vec![core_id])),Arc::new(RwLock::new(vec![thread_id])),query_rx,query_results);
                            let processor=processor.add_to_monitor();
                            println!("Processor no.{}running...",processor_id);
                            processor.process_queries();
                            // thread_id
                        });
                        handles.push(ThreadData { processor_id, handle });
                    }
                }
                sleep(Duration::from_millis(100));
                println!("All processors up and running.");
                // Join all threads
                for thread_data in handles {
                    match thread_data.handle.join() {
                        Ok(_) => println!("Processor no. {} Terminated", thread_data.processor_id),
                        Err(e) => eprintln!("Processor no. {}'s main thread terminated due to a panic: {:?}", thread_data.processor_id, e),
                    }
                }
                if let Err(e) = tx_processors_shutdown.send("All processors have been shut down.".to_string()) {
                    eprintln!("Failed to send shutdown notification: {}", e);
                }
            }
            EngineMode::MultiCoreMultipleThreadsEachThreadInEachCoreSearchingAgainstSingleShardedSubsetOfIndex => {
                let mut handles: Vec<ThreadData> = Vec::new();
                // Get the number of physical cores
                let physical_cores = num_cpus::get_physical();
                let core_ids = core_affinity::get_core_ids().unwrap();
                let mut search_library = SearchLibrary::new();
                let docs_clone = documents.clone();
                for doc in docs_clone {
                    search_library.add_document_to_index(&doc);
                }
                println!("Search Library Constructed.");
                let search_library = Arc::new(RwLock::new(search_library));
                for i in 0..physical_cores {
                    let processor_id = i+1;
                    let query_rx = query_rx.clone();
                    let query_results = Arc::clone(&query_results);
                    let search_library = Arc::clone(&search_library);
                    let core_id = core_ids[i];
                    let handle = thread::spawn(move || {
                        // Pin this thread to the specific core
                        core_affinity::set_for_current(core_id);
                        let thread_id = thread::current().id();
                        let processor = Processor::new(processor_id,search_library,Arc::new(RwLock::new(vec![core_id])),Arc::new(RwLock::new(vec![thread_id])),query_rx,query_results);
                        let processor=processor.add_to_monitor();
                        println!("Processor no.{} running...",processor_id);
                        processor.process_each_query_across_all_threads_in_a_core(core_id);
                        // thread_id
                    });
        
                    handles.push(ThreadData { processor_id, handle });

                }
                sleep(Duration::from_millis(100));
                println!("All processors up and running.");
                // Join all threads
                for thread_data in handles {
                    match thread_data.handle.join() {
                        Ok(_) => println!("Processor no. {} Terminated", thread_data.processor_id),
                        Err(e) => eprintln!("Processor no. {}'s main thread terminated due to a panic: {:?}", thread_data.processor_id, e),
                    }
                }
                if let Err(e) = tx_processors_shutdown.send("All processors have been shut down.".to_string()) {
                    eprintln!("Failed to send shutdown notification: {}", e);
                }
            }
        }
    }
}

// Function to listen for user queries and add them to the queue
fn listen_for_user_queries(query_tx: channel::Sender<QueryChannelSenderMessage>,query_results: Arc<QueryResults>) {
    sleep(Duration::from_millis(1000));
    loop {
        // Prompt the user
        println!("\nEnter your query: ");
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
        query_tx.send(QueryChannelSenderMessage::Query(query)).unwrap();
        
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
                    sleep(Duration::from_millis(100));
                }
            }
        }
    }
}

fn main() {
    // This should initialize the Monitor object contained in global MONITOR lazy static singleton
    initialize_shared_monitor();
    match  unsafe { fork() } {
        Ok(ForkResult::Parent { child, .. }) => {
            println!("Monitoring process PID: {}", child);
            // Continue with main program logic...
            // Example documents 
            // let doc1 = Document::new("doc1", DocumentFormat::PlainText("Rust is a systems programming language.".into()));
            // let doc2 = Document::new("doc2", DocumentFormat::PlainText("Search engines are essential for the web.".into()));
            // let documents = vec![doc1, doc2];

            // Collect the command-line arguments
            let args: Vec<String> = env::args().collect();

            if args.len() < 4 {
                eprintln!("Usage: {} <documents_folder> <engine_mode> <mode>", args[0]);
                eprintln!("Engine Modes: single_core_single_thread, multi_core_single_thread, multi_core_multiple_threads_each_thread_searching_against_whole_index, multi_core_multiple_threads_each_thread_in_each_core_searching_against_single_sharded_subset_of_index");
                return;
            }


            let documents_folder = &args[1];

            let mode_str = &args[2];
            let mode: EngineMode = match EngineMode::from_str(mode_str) {
                Some(m) => m,
                None => {
                    eprintln!("Invalid engine mode: {}", mode_str);
                    return;
                }
            };

            let run_mode = &args[3];
            if run_mode != "testing" && run_mode != "production" {
                eprintln!("Invalid mode: {}. Use 'testing' or 'production'.", run_mode);
                return;
            }

            let mut documents = Vec::new();

            for entry in fs::read_dir(documents_folder).expect("Failed to read documents directory") {
                let entry = entry.expect("Failed to read directory entry");
                let path = entry.path();

                if path.is_file() {
                    let file_extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                    // let file_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
                    let file_name_with_extension = path.file_name().and_then(|s| s.to_str()).unwrap_or("unknown"); // Include extension in the file name
                    let mut file_content = String::new();
                    let mut file = fs::File::open(&path).expect("Failed to open file");
                    file.read_to_string(&mut file_content).expect("Failed to read file");

                    let document_format: DocumentFormat = match file_extension {
                        "txt" => DocumentFormat::PlainText(file_content),
                        "json" => DocumentFormat::Json(file_content), // Store raw JSON string
                        "html" => DocumentFormat::Html(file_content),
                        _ => {
                            println!("Skipping unsupported file type: {}", file_extension);
                            continue;
                        }
                    };

                    let document = Document::new(file_name_with_extension, document_format);
                    documents.push(document);
                }
            }

            // Now you have all the documents read and parsed into the `documents` vector.
            // You can proceed with creating the search index.

            println!("Loaded {} documents", documents.len());

            // Create a new query queue
            let (query_tx, query_rx): (channel::Sender<QueryChannelSenderMessage>, channel::Receiver<QueryChannelSenderMessage>) = channel::unbounded();
            let query_rx_clone: channel::Receiver<QueryChannelSenderMessage> = query_rx.clone();
            let query_results = Arc::new(QueryResults::new());

            if run_mode == "production" {
                // Run the listen_for_user_queries function on a separate thread in production mode
                let query_tx_clone: channel::Sender<QueryChannelSenderMessage> = query_tx.clone();
                let query_results_clone = Arc::clone(&query_results);
                thread::spawn(move || {
                    listen_for_user_queries(query_tx_clone, query_results_clone);
                });
            }

                // Start the async server
                let server_query_tx_clone: channel::Sender<QueryChannelSenderMessage> = query_tx.clone();
                thread::spawn(move || {
                    tokio::runtime::Runtime::new().unwrap().block_on(server::run_query_requests_server(server_query_tx_clone)).unwrap();
                });
                let server_query_results_clone = Arc::clone(&query_results);
                thread::spawn(move || {
                    tokio::runtime::Runtime::new().unwrap().block_on(server::run_get_results_server(server_query_results_clone)).unwrap();
                });

            let query_results_clone = Arc::clone(&query_results);
            // let mode = EngineMode::SingleCoreSingleThread; // Set the desired mode here
            let engine= Engine::new(mode);
            let (tx_processors_shutdown, rx_processors_shutdown): (Sender<String>, Receiver<String>) = mpsc::channel();

            // The main thread can do other tasks, or join the spawned thread if necessary

            let (tx_signal, rx_signal) = std::sync::mpsc::channel();
            let signal_handler = Arc::new(Mutex::new(tx_signal));
            let signal_handler_clone = signal_handler.clone();
                // Spawn a thread to handle signals
            thread::spawn(move || {
                let mut signals = Signals::new(&[SIGINT, SIGTERM]).unwrap();
                // Listen for SIGINT
                for sig in signals.forever() {
                    match sig {
                        SIGINT => {
                            println!("Received SIGINT, shutting down gracefully...");
                            // Perform any cleanup or shutdown tasks here
                            kill(child, Signal::SIGTERM).unwrap();
                            signal_handler_clone.lock().unwrap().send(()).unwrap();
                            break;
                        }
                        SIGTERM => {
                            println!("Received SIGTERM, shutting down gracefully...");
                            kill(child, Signal::SIGTERM).unwrap();
                            signal_handler_clone.lock().unwrap().send(()).unwrap();
                            break;
                        }
                        _ => unreachable!(),
                    }
                    
                }
            });
            let monitor_query_tx_clone:channel::Sender<QueryChannelSenderMessage> = query_tx.clone();
            thread::spawn(move || {
                // Wait for SIGINT signal
                rx_signal.recv().unwrap();
                // Shut down all processors
                let monitor = MONITOR.lock().unwrap();
                monitor.shutdown_all_processors(monitor_query_tx_clone);
                match rx_processors_shutdown.recv() {
                    Ok(message) => println!("{}", message),
                    Err(e) => eprintln!("Failed to receive shutdown message: {}", e),
                }
            });
            let monitor = MONITOR.lock().unwrap();
            println!("Starting monitor...");
            monitor.start_monitoring();
            std::mem::drop(monitor);
            println!("Starting Engine...");
            println!("Do you wish to shut down? (Press Ctrl+C to exit or send SIGINT signal): ");
            engine.start_engine(documents,query_rx_clone,query_results_clone,tx_processors_shutdown.clone());
            // Wait for the child process to terminate
            match waitpid(child, None) {
                Ok(WaitStatus::Exited(pid, status)) => {
                    println!("Child process (PID: {}) exited with status: {}", pid, status);
                }
                Ok(WaitStatus::Signaled(pid, signal, _)) => {
                    println!("Child process (PID: {}) was terminated by signal: {:?}", pid, signal);
                }
                Ok(_) => {
                    println!("Child process (PID: {}) had an unexpected status", child);
                }
                Err(e) => {
                    eprintln!("Error waiting for child process: {}", e);
                }
            }
            unsafe {
                cleanup_shared_memory(SHM_PTR, SHM_SIZE, true); // unmap and unlink
            }
            println!("Search Engine Shutdown complete.");
            exit(0); 
        }
        Ok(ForkResult::Child) => {
            println!("Starting monitoring process...");
            start_logging_monitor_info();
            // Child cleans up shared memory
            unsafe {
                cleanup_shared_memory(SHM_PTR, SHM_SIZE, false); // Only unmap
            }

            // Exit child process
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("Fork failed: {}", e);
            exit(1);
        }
    }

}
