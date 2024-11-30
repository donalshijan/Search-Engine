use std::collections::HashMap;
use std::panic;
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
    query_rx: channel::Receiver<Query>,
    query_results: Arc<QueryResults>,
    last_heartbeat: Arc<Mutex<Instant>>,
    is_alive: Arc<AtomicBool>,
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
    pub fn new(id:usize,search_library: Arc<RwLock<SearchLibrary>>,core_ids: Arc<RwLock<Vec<CoreId>>>, thread_ids: Arc<RwLock<Vec<ThreadId>>>,query_rx: channel::Receiver<Query>,query_results: Arc<QueryResults>) -> Self{
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

        loop {
            let result = panic::catch_unwind(|| {
                // Fetch a query from the queue
                match self.query_rx.recv() {
                    Ok(query) => {
                        let query_result = {
                            let search_library = self.search_library.read().unwrap();
                            search_library.search(&query,None)
                        };
                        self.query_results.insert(query.id.clone(), query_result);
                    }
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }

                let mut last_heartbeat_lock = self.last_heartbeat.lock().unwrap();
                *last_heartbeat_lock = Instant::now();

                if !self.is_alive.load(Ordering::SeqCst) {
                    return Err("Processor stopped".to_string());
                }

                Ok(())
            });

            match result {
                Ok(_) => continue, // Keep processing queries
                Err(err) => {
                    // Downcast the error to the type you're expecting
                    if let Some(err_str) = err.downcast_ref::<String>() {
                        if err_str == "Processor stopped" {
                            break; // Gracefully stop the thread
                        }
                    }
                    // Panic if any other error occurs
                    panic!("Error in process_queries: {:?}", err);
                }
            }
        }
    }

    pub fn process_each_query_across_all_threads_in_a_core(&self,core_id: CoreId){
        let result_store: Arc<Mutex<QueryResult>>=Arc::new(Mutex::new(QueryResult::new("".to_string(),"".to_string(),vec![("".to_string(),0)],Duration::ZERO)));
        let current_item: Arc<(Mutex<Option<Query>>, Condvar)> = Arc::new((Mutex::new(None), Condvar::new()));
        let num_cores = core_affinity::get_core_ids().unwrap().len();
        let total_threads = num_cpus::get();
        let num_threads_per_core = if num_cores > 0 {
                    total_threads / num_cores
                } else {
                    1 // Default to 1 if no cores are found
                };
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
            let current_item_clone = Arc::clone(&current_item);
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
                thread_ids.push(thread::current().id());
                loop {
                    let result = panic::catch_unwind(|| {
                        let (query_item, cvar) = &*current_item_clone;
                        let mut query_item_opt = query_item.lock().unwrap();
        
                        while query_item_opt.is_none() {
                            query_item_opt = cvar.wait(query_item_opt).unwrap();
                        }
        
                        if let Some(ref query) = *query_item_opt {
                            // Wait for all threads to reach this point
                            barrier_clone.wait();
                            // Scope the search_library lock to ensure it's released before the second barrier
                            let result = {
                                let search_library = search_library.read().unwrap();  // (2)
                                search_library.search(query,Some(key_range))
                            };  // Lock is dropped here when search_library goes out of scope
                            let mut old_result = result_store_clone.lock().unwrap();
                            let updated_result = old_result.aggregate_result(result).clone();
                            std::mem::drop(old_result);
                            barrier_clone.wait(); // Wait for all threads to finish processing
                            query_results_clone.insert(updated_result.clone().query_id, updated_result)
                        }
                        // Signal that this thread has finished processing
                        *query_item_opt = None;  // Reset the item
                        std::mem::drop(query_item_opt);
                        cvar.notify_all(); // Notify other threads to check for the next item
                        
                        let mut last_heartbeat_lock = last_heartbeat.lock().unwrap();
                        *last_heartbeat_lock = Instant::now();

                        if !is_alive.load(Ordering::SeqCst) {
                            return Err("Processor stopped".to_string());
                        }

                        Ok(())
                    });

                    match result {
                        Ok(_) => continue, // Keep processing queries
                        Err(err) => {
                            // Downcast the error to the type you're expecting
                            if let Some(err_str) = err.downcast_ref::<String>() {
                                if err_str == "Processor stopped" {
                                    break; // Gracefully stop the thread
                                }
                            }
                            // Panic if any other error occurs
                            panic!("Error in processor's sub thread: {:?}", err);
                        }
                    }
                }
                return thread::current().id()
            });
            handles.push(ThreadData{thread_no:i+1,handle});
        }
        // Join all threads
        let (tx, rx) = mpsc::channel();

        // Spawn a monitor thread
        // let handles_monitor = handles.clone(); 
        let tx_monitor = tx.clone();
        thread::spawn(move || {
            for thread_data in handles {
                match thread_data.handle.join() {
                    Ok(_) => {println!("Sub Thread no. {} of processor no. {} Terminated", thread_data.thread_no,processor_id);
                    let message = format!("Sub thread no. {} of processor no. {} terminated", thread_data.thread_no,processor_id);
                    tx_monitor.send(message).unwrap();
                    }
                    Err(e) => {
                        let panic_info = format!("{:?}", e);
                        eprintln!("Sub Thread no. {} of processor no. {} panicked: {}", thread_data.thread_no, processor_id, panic_info);
                        let message = format!("Sub thread no. {} of processor no. {} panicked: {}", thread_data.thread_no, processor_id, panic_info);
                        tx_monitor.send(message).unwrap();
                    }
                }
            }
        });
        //main thread which pulls one request from the query queue and passes it to each thread to process in parallel
        let query_rx_clone =  self.query_rx.clone();
        let  last_heartbeat = self.last_heartbeat.clone();
        let  is_alive = self.is_alive.clone();
        loop {
            let result = panic::catch_unwind(|| {
                match query_rx_clone.recv() {
                    Ok(query) => {
                        let (lock, cvar) = &*current_item;
                        let mut current_item_lock = lock.lock().unwrap();
                        *current_item_lock = Some(query);
                        cvar.notify_all(); // Notify threads that an item is ready to be processed
        
                        // Wait until all threads have finished processing
                        while current_item_lock.is_some() {
                            current_item_lock = cvar.wait(current_item_lock).unwrap();
                        }
                    }
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
                let mut last_heartbeat_lock = last_heartbeat.lock().unwrap();
                *last_heartbeat_lock = Instant::now();

                if !is_alive.load(Ordering::SeqCst) {
                    return Err("Processor stopped".to_string());
                }

                Ok(())
            });
            match result {
                Ok(_) => continue, // Keep processing queries
                Err(err) => {
                    for msg in rx {
                        println!("Processor's main thread received Message: {}", msg);
                    }
                    // Downcast the error to the type you're expecting
                    if let Some(err_str) = err.downcast_ref::<String>() {
                        if err_str == "Processor stopped" {
                            break; // Gracefully stop the thread
                        }
                    }
                    // Panic if any other error occurs
                    panic!("Error in processor's main thread: {:?}", err);
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

    pub fn shutdown_all_processors(&self) {
        // Clone the processors Arc to avoid holding the lock longer than necessary
        let processors = self.processors.clone();
    
        // Iterate over all processors and set is_alive to false
        for processor in processors.iter() {
            processor.is_alive.store(false, Ordering::SeqCst);
        }
    
        // Wait a moment to ensure all processors stop
        thread::sleep(Duration::from_millis(100));
    }

    pub fn start_monitoring(&self) {
        let processors = self.processors.clone();
        thread::spawn(move || {
            loop {
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

                // Sleep before the next update
                thread::sleep(Duration::from_secs(5));
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