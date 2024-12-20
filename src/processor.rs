pub mod processor {
use std::collections::HashMap;
use std::{panic, usize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Barrier, Condvar, Mutex, RwLock};
use std::{sync::Arc, thread::ThreadId};
use std::time::{Duration, Instant};
use std::thread::{self};
use core_affinity::CoreId;
use prettytable::{Cell, Row, Table};
use search_engine::{Query,  QueryResult, QueryResults, SearchLibrary};
use lazy_static::lazy_static;
use crossbeam::channel;
use search_engine::QueryChannelSenderMessage;


// lazy_static! {
//     static ref PROCESSORS: Mutex<Vec<Arc<Processor>>> = Mutex::new(Vec::new());
// }

lazy_static! {
    pub static ref MONITOR: Arc<Mutex<Monitor>> = Arc::new(Mutex::new(Monitor {
        processors: Vec::new(),
    }));
}

struct ThreadData {
    thread_no: usize,
    handle: thread::JoinHandle<ThreadId>,
}

#[derive(Clone)]
pub struct Processor{
    id: usize,
    search_library:Arc<RwLock<SearchLibrary>>,
    core_ids: Arc<RwLock<Vec<CoreId>>>,
    thread_ids: Arc<RwLock<Vec<ThreadId>>>,
    query_rx: channel::Receiver<QueryChannelSenderMessage>,
    query_results: Arc<QueryResults>,
    last_heartbeat: Arc<Mutex<Instant>>,
    is_alive: Arc<AtomicBool>,
}


#[derive(Clone,Debug)]
enum ProcessorMessage {
    Query(Option<Query>),
    StopMessage(String)
}

fn core_id_to_string(core_id: CoreId) -> String {
    // Convert CoreId to a displayable format, e.g., integer or string
    format!("{:?}", core_id)
}

fn thread_id_to_string(thread_id: thread::ThreadId) -> String {
    // Convert ThreadId to a displayable format
    format!("{:?}", thread_id) // or some other suitable representation
}

impl Processor{
    pub fn new(id:usize,search_library: Arc<RwLock<SearchLibrary>>,core_ids: Arc<RwLock<Vec<CoreId>>>, thread_ids: Arc<RwLock<Vec<ThreadId>>>,query_rx: channel::Receiver<QueryChannelSenderMessage>,query_results: Arc<QueryResults>) -> Self{
        Processor {
            id,
            search_library,
            core_ids,
            thread_ids,
            query_rx,
            query_results,
            last_heartbeat: Arc::new(Mutex::new(Instant::now())),
            is_alive: Arc::new(AtomicBool::new(true)),
        }
    }
    pub fn add_new_thread_for_processor(&mut self, new_thread_id: ThreadId) {
        // Directly push the new thread ID into the existing Vec<ThreadId>
        self.thread_ids.write().unwrap().push(new_thread_id);
    }
    pub fn add_to_monitor(self) -> Arc<Self> {
        let processor = Arc::new(self);
        add_new_processor(processor.clone());
        processor
    }
    pub fn process_queries(&self){

        // Set up a custom panic hook to log errors or clean up resources if needed
        panic::set_hook(Box::new(|panic_info| {
            eprintln!("Thread panicked: {:?}", panic_info);
            // Additional logging or cleanup can be done here
        }));
        let result = panic::catch_unwind(|| -> Result<(), Box<dyn std::any::Any + Send>> {
            loop {
                let result: Result<(), String> = {
                    // Fetch a query from the queue
                    match self.query_rx.recv() {
                        Ok(QueryChannelSenderMessage::Query(query)) => {
                            let query_result = {
                                let search_library = self.search_library.read().unwrap();
                                search_library.search(&query,None)
                            };
                            self.query_results.insert(query.id.clone(), query_result);
                        }
                        Ok(QueryChannelSenderMessage::Stop(message)) => {
                            println!("Received stop signal: {}", message);
                        }
                        Err(_) => {
                            std::thread::sleep(Duration::from_millis(100));
                        }
                    }
                    let mut last_heartbeat_lock = self.last_heartbeat.lock().unwrap();
                    *last_heartbeat_lock = Instant::now();
                    if !self.is_alive.load(Ordering::SeqCst) {
                        Err("Processor stopped".to_string())
                    }
                    else{
                        Ok(())
                    }
                    
                };
    
                match result {
                    Ok(_) => {
                        continue;
                    }, // Keep processing queries
                    Err(err) => {
                        if err == "Processor stopped" {
                            break; // Gracefully stop the thread
                        }
                        // Propagate panic explicitly
                        std::panic::resume_unwind(Box::new(err));
                    }
                }
            }
            Ok(())
        });
        match result {
            Ok(_) => {
                println!("Processor shutting down ...");
            }
            Err(err) => {
                // Panic if any other unexpected error occurs
                panic!("Error in process_queries: {:?}", err);
            }
        }
    }

    pub fn process_each_query_across_all_threads_in_a_core(&self,core_id: CoreId){
        let result_store: Arc<Mutex<QueryResult>>=Arc::new(Mutex::new(QueryResult::new("".to_string(),"".to_string(),vec![("".to_string(),0)],Duration::ZERO)));
        let current_processor_message_item: Arc<(Mutex<Option<ProcessorMessage>>, Condvar)> = Arc::new((Mutex::new(None), Condvar::new()));
        let num_of_sub_threads_finished_processing:Arc<(Mutex<Option<usize>>, Condvar)> = Arc::new((Mutex::new(Some(0)),Condvar::new()));
        let num_cores = num_cpus::get_physical();
        let total_threads = num_cpus::get();
        let num_threads_per_core_recommended = if num_cores > 0 {
                    total_threads / num_cores
                } else {
                    1 // Default to 1 if no cores are found
                };
        let mut num_threads_per_core = num_threads_per_core_recommended;
        if num_threads_per_core_recommended == 1{
            num_threads_per_core=2;
        }
        let barrier = Arc::new(Barrier::new(num_threads_per_core));
        
        let thread_ids: Vec<ThreadId> = Vec::new();
        let mut handles: Vec<ThreadData>=Vec::new();
        let processor_id = self.id.clone();
        panic::set_hook(Box::new(|panic_info| {
            eprintln!("Thread panicked: {:?}", panic_info);
            // Additional logging or cleanup can be done here
        }));
        for i in 0..num_threads_per_core {
            let result_store_clone = Arc::clone(&result_store);
            let current_processor_message_item_clone = Arc::clone(&current_processor_message_item);
            let num_of_sub_threads_finished_processing_clone = Arc::clone(&num_of_sub_threads_finished_processing);
            // Extract a subset of the index for this thread
            let search_library =  self.search_library.clone();
            let keys: Vec<String> = search_library.read().unwrap().index.keys().cloned().collect();
            let shard_size = keys.len() / num_threads_per_core;
            let start = i * shard_size;
            let end = ((i + 1) * shard_size).min(keys.len());
            // let subset_shard_of_original_index: HashMap<String, Vec<String>> = keys[start..end]
            // .iter()
            // .filter_map(|key| original_index.get(key).map(|value| (key.clone(), value.clone())))
            // .collect();

            // let search_library_clone = Arc::new(RwLock::new(SearchLibrary::create_index_from_existing_index(subset_shard_of_original_index)));
            let query_results_clone =  self.query_results.clone();
            let barrier_clone = Arc::clone(&barrier);
            let mut thread_ids = thread_ids.clone();
            let  last_heartbeat = self.last_heartbeat.clone();
            let  is_alive = self.is_alive.clone();
            core_affinity::set_for_current(core_id);
            let handle = thread::spawn(move || {
                let key_range = &keys[start..end]; 
                let thread_id = thread::current().id();
                thread_ids.push(thread::current().id());
                println!("Sub thread:{:?} created for processor no. {}",thread::current().id(),processor_id);
                loop {
                    let result:Result<(), String> =  {
                        let (processor_message_opt_lock, cvar) = &*current_processor_message_item_clone;
                        let mut processor_message_opt = processor_message_opt_lock.lock().unwrap();
                        while processor_message_opt.is_none() {
                            processor_message_opt = cvar.wait(processor_message_opt).unwrap();
                        }
                        // Take a copy of the Option<ProcessorMessage> inside the processor_message_opt so that lock can be released for other threads to process as well
                        let processor_message_cloned = processor_message_opt.clone();  
                        std::mem::drop(processor_message_opt); // Explicitly drop the lock
                        if let Some(ref processor_message) = processor_message_cloned {
                            match processor_message {
                                ProcessorMessage::Query(Some(ref query)) => {
                                    // Wait for all threads to reach this point
                                    barrier_clone.wait();
                                    // Scope the search_library lock to ensure it's released before the second barrier
                                    let result = {
                                        let search_library = search_library.read().unwrap(); // (2)
                                        search_library.search(query, Some(key_range))
                                    }; // Lock is dropped here when search_library goes out of scope
                                    let mut old_result = result_store_clone.lock().unwrap();
                                    let updated_result = old_result.aggregate_result(result).clone();
                                    query_results_clone.insert(updated_result.clone().query_id, updated_result);
                                    std::mem::drop(old_result);
                                    barrier_clone.wait(); // Wait for all threads to finish processing
                                    {
                                        // clear the recently processed and final aggregated result held in the shared QueryResult instance so that it can be used for processing other queries later
                                        let mut old_result = result_store_clone.lock().unwrap();
                                        old_result.query_id.clear();
                                    }
                                }
                                ProcessorMessage::StopMessage(message) => {
                                    println!("Shutting down: {}", message);
                                }
                                _ => {
                                    println!("Received an unprocessable message.");
                                }
                            }
                        
                        }
                        // Signal that this thread has finished processing
                        barrier_clone.wait();
                        let mut processor_message_opt = processor_message_opt_lock.lock().unwrap();
                        *processor_message_opt = None;  // Reset the item
                        cvar.notify_all(); // Notify other threads to check for the next item
                        std::mem::drop(processor_message_opt);
                        let (num_of_sub_threads_finished_processing_lock,cvar) =  &*num_of_sub_threads_finished_processing_clone;
                        let mut num_of_sub_threads_finished_processing_option = num_of_sub_threads_finished_processing_lock.lock().unwrap(); 
                        let current_num_of_finished_threads = *num_of_sub_threads_finished_processing_option;
                        let updated_num_of_finished_threads = match current_num_of_finished_threads{
                            Some(num) => {
                                num+1
                            }
                            None => {
                                1
                            }
                        };
                        *num_of_sub_threads_finished_processing_option = Some(updated_num_of_finished_threads);
                        cvar.notify_all();
                        std::mem::drop(num_of_sub_threads_finished_processing_option);
                        barrier_clone.wait();
                        let mut last_heartbeat_lock = last_heartbeat.lock().unwrap();
                        *last_heartbeat_lock = Instant::now();
                        if !is_alive.load(Ordering::SeqCst) {
                            Err("Processor stopped".to_string())
                        }
                        else{
                            Ok(())
                        }
                    };

                    match result {
                        Ok(_) => continue, // Keep processing queries
                        Err(err) => {
                            // Downcast the error to the type you're expecting
                                if err == "Processor stopped" {
                                    break thread_id; // Gracefully stop the thread
                                }
                            // Panic if any other error occurs
                            panic!("Error in processor's sub thread: {:?}", err);
                        }
                    }
                }
            });
            handles.push(ThreadData{thread_no:i+1,handle});
        }
        // Join all threads
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            for thread_data in handles {
                match thread_data.handle.join() {
                    Ok(_) => {
                        println!("Sub Thread no. {} of processor no. {} Terminated", thread_data.thread_no,processor_id);
                        let message = format!("Sub thread no. {} of processor no. {} Terminated", thread_data.thread_no,processor_id);
                        tx.send(message).unwrap();
                    }
                    Err(e) => {
                        let panic_info = format!("{:?}", e);
                        eprintln!("Sub Thread no. {} of processor no. {} panicked: {}", thread_data.thread_no, processor_id, panic_info);
                        let message = format!("Sub thread no. {} of processor no. {} panicked: {}", thread_data.thread_no, processor_id, panic_info);
                        tx.send(message).unwrap();
                    }
                }
            }
        });
        //main thread which pulls one request from the query queue and passes it to each thread to process in parallel
        let query_rx_clone =  self.query_rx.clone();
        let  last_heartbeat = self.last_heartbeat.clone();
        let  is_alive = self.is_alive.clone();
        loop {
            let result = {
                match query_rx_clone.recv() {
                    Ok(QueryChannelSenderMessage::Query(query)) => {
                        let (lock, cvar) = &*current_processor_message_item;
                        let mut current_processor_message_item_option = lock.lock().unwrap();
                        *current_processor_message_item_option = Some(ProcessorMessage::Query(Some(query)));
                        cvar.notify_all(); // Notify threads that an item is ready to be processed
                        std::mem::drop(current_processor_message_item_option);
                        // Wait until all threads have finished processing
                        let (lock, cvar) = &*num_of_sub_threads_finished_processing;
                        let mut num_of_sub_threads_finished_processing_option = lock.lock().unwrap();
                        while num_of_sub_threads_finished_processing_option.is_some_and(|n| n!=num_threads_per_core) {
                            num_of_sub_threads_finished_processing_option = cvar.wait(num_of_sub_threads_finished_processing_option).unwrap();
                        }
                        *num_of_sub_threads_finished_processing_option = Some(0);
                        std::mem::drop(num_of_sub_threads_finished_processing_option);
                    }
                    Ok(QueryChannelSenderMessage::Stop(message)) => {
                        println!("Received stop signal: {}", message);
                        let (lock, cvar) = &*current_processor_message_item;
                        let mut current_processor_message_item_option = lock.lock().unwrap();
                        *current_processor_message_item_option = Some(ProcessorMessage::StopMessage(message));
                        cvar.notify_all(); // Notify threads that an item is ready to be processed
                        // Wait until all threads have finished processing
                        while current_processor_message_item_option.is_some() {
                            current_processor_message_item_option = cvar.wait(current_processor_message_item_option).unwrap();
                        }
                    }
                    Err(err) => {
                        println!("Error:{}",err);
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
                let mut last_heartbeat_lock = last_heartbeat.lock().unwrap();
                *last_heartbeat_lock = Instant::now();

                if !is_alive.load(Ordering::SeqCst) {
                    Err("Processor stopped".to_string())
                }
                else{
                    Ok(())
                }
            };
            match result {
                Ok(_) => {
                    continue}, // Keep processing queries
                Err(err) => {
                    for msg in rx {
                        println!("Processor's main thread received Message: {}", msg);
                    }
                    if err == "Processor stopped" {
                        break; // Gracefully stop the thread
                    }
                    // Panic if any other error occurs
                    else{
                        panic!("Error in processor's main thread: {:?}", err);
                    }
                }
            }
        }

    }

    // pub fn core_ids(&self) -> &Vec<CoreId> {
    //     &self.core_ids
    // }

    // pub fn thread_ids(&self) -> &Vec<ThreadId> {
    //     &self.thread_ids.
    // }


    pub fn is_alive(&self) -> bool {
        let last_heartbeat = self.last_heartbeat.lock().unwrap();
        last_heartbeat.elapsed() < Duration::from_secs(20) && Arc::strong_count(&self.search_library) > 0 && self.is_alive.load(Ordering::SeqCst)
    }

    pub fn restart(&self) {
        // Implement the logic to restart the processor
    }
}

pub struct Monitor {
    processors: Vec<Arc<Processor>>,
}

impl Monitor {
    pub fn add_processor(&mut self, processor: Arc<Processor>) {
        self.processors.push(processor);
    }
    
    pub fn kill_processor(&self, processor_id: usize) {
        // Iterate over all processors and kill the one with the matching ID
        let processors = self.processors.clone();
        for processor in processors.iter() {
            if processor.id == processor_id {
                processor.is_alive.store(false, Ordering::SeqCst);

                // Wait a moment to ensure the processor stops
                thread::sleep(Duration::from_millis(100));
            }
        }
    }

    pub fn shutdown_all_processors(&self, query_tx: channel::Sender<QueryChannelSenderMessage>) {
        // Clone the processors Arc to avoid holding the lock longer than necessary
        let processors = self.processors.clone();
    
        // Iterate over all processors and set is_alive to false
        for processor in processors.iter() {
            processor.is_alive.store(false, Ordering::SeqCst);
        }
        // Step 2: Send a "stop" message for each processor
        let num_processors = processors.len();
        for _ in 0..num_processors {
            // Sending a stop signal to each processor
            if let Err(err) = query_tx.send(QueryChannelSenderMessage::Stop("STOP".to_string())) {
                eprintln!("Failed to send stop signal: {:?}", err);
            }
        }
        // Wait a moment to ensure all processors stop
        thread::sleep(Duration::from_millis(100));
    }

    pub fn start_monitoring(&self) {
        thread::spawn(move || {
            loop {
                // Access MONITOR singleton directly within the loop
                let monitor = MONITOR.lock().unwrap();
                let processors = monitor.processors.clone();
                std::mem::drop(monitor);
                {
                    // Acquire a write lock to modify the processors list
                    let mut processors = processors.clone();
                    
                    // Retain only those processors that are alive
                    processors.retain(|processor| processor.is_alive());
                }
                let mut table = Table::new();
                
                // Header for the table
                table.add_row(Row::new(vec![
                    Cell::new("Type"),
                    Cell::new("Id"),
                    // Cell::new("CPU Usage (%)"),
                    // Cell::new("Memory Usage (bytes)"),
                    Cell::new("Processor No./No.s"),
                ]));
                
                // Group processors by core
                let mut core_data: HashMap<CoreId, Vec<Arc<Processor>>> = HashMap::new();
                for processor in &processors {
                        // one more loop on core_ids
                    for core_id in processor.core_ids.read().unwrap().clone(){
                        core_data.entry(core_id.clone())
                        .or_insert_with(Vec::new)
                        .push(Arc::clone(processor));
                    }
                    
                }

                // Sort and display core and thread information
                for (core_id, processors_in_core) in core_data {
                    // Core information (aggregated CPU and memory usage for all threads on this core)
                    // let core_cpu_usage: f64 = processors_in_core.iter().map(|p| p.cpu_usage()).sum();
                    // let core_memory_usage: usize = processors_in_core.iter().map(|p| p.memory_usage()).sum();
                    // let mut processor_threads: Vec<ThreadId> = Vec::new();
                    // for processor in processors_in_core.clone() {
                    //     for thread_id in processor.thread_ids.read().unwrap().clone(){
                    //         processor_threads.push(thread_id);
                    //     }
                    // }
                    // let core_cpu_usage: f64 = cpu_usage(processor_threads.clone());
                    // let core_memory_usage: usize = memory_usage(processor_threads);
                    let processor_ids: String = processors_in_core
                    .iter()                                 
                    .map(|processor| processor.id.to_string()) 
                    .collect::<Vec<String>>()               
                    .join(","); 
                    // Add core row to table
                    table.add_row(Row::new(vec![
                        Cell::new("Core"),
                        Cell::new(&core_id_to_string(core_id)),
                        // Cell::new(&format!("{:.2}", core_cpu_usage)),
                        // Cell::new(&core_memory_usage.to_string()),
                        Cell::new(&processor_ids),
                    ]));
                    
                    // Add thread rows associated with this core
                    for processor in processors_in_core {
                        for thread_id in processor.thread_ids.read().unwrap().clone(){
                            table.add_row(Row::new(vec![
                                Cell::new("Thread"),
                                Cell::new(&thread_id_to_string(thread_id.clone())),
                                // Cell::new(&format!("{:.2}", cpu_usage(vec![thread_id]))),
                                // Cell::new(&memory_usage(vec![thread_id]).to_string()),
                                Cell::new(&processor.id.to_string())
                            ]));
                        }
                        
                    }
                }

                // Clear the terminal screen and print the table at the bottom
                print!("\x1B[2J\x1B[1;1H"); // Clear the screen
                table.printstd();
                println!("Do you wish to shut down? (Press Ctrl+C to exit or send SIGINT signal): ");
                println!("\nEnter your query: ");
                // Sleep before the next update
                thread::sleep(Duration::from_secs(10));
            }
        });
    }
}

fn add_new_processor(processor: Arc<Processor>) {
    let mut monitor = MONITOR.lock().unwrap();
    monitor.add_processor(processor);
}

// Example method to get memory usage
// pub fn memory_usage(ids: Vec<ThreadId>) -> usize {
//     // Dummy implementation
//     // You should replace this with actual memory usage calculation
//     0
// }

// pub fn cpu_usage(ids: Vec<ThreadId>) -> f64 {
//     // Implement CPU usage calculation for the thread
//     20.0 // Example value, replace with actual implementation
// }
}