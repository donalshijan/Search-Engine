use std::sync::{Barrier, Condvar, Mutex, RwLock};
use std::{sync::Arc, thread::ThreadId};
use std::time::Duration;
use std::thread;
use core_affinity::CoreId;
use search_engine::{Query,  QueryResult, QueryResults, SearchLibrary};
use lazy_static::lazy_static;
use std::time::UNIX_EPOCH;
use crossbeam::channel;


lazy_static! {
    static ref PROCESSORS: Mutex<Vec<Arc<Processor>>> = Mutex::new(Vec::new());
}

#[derive(Clone)]
pub struct Processor{
    search_library:Arc<RwLock<SearchLibrary>>,
    core_id: CoreId,
    thread_id: ThreadId,
    query_rx: channel::Receiver<Query>,
    query_results: Arc<QueryResults>,
}


impl Processor{
    pub fn new(search_library: Arc<RwLock<SearchLibrary>>,core_id: CoreId, thread_id: ThreadId,query_rx: channel::Receiver<Query>,query_results: Arc<QueryResults>) -> Self{
        Processor {
            search_library,
            core_id,
            thread_id,
            query_rx,
            query_results
        }
    }
    pub fn store_in_global(self) -> Arc<Self> {
        let processor = Arc::new(self);
        PROCESSORS.lock().unwrap().push(Arc::clone(&processor));
        processor
    }
    pub fn process_queries(&self){
        let query_rx_clone = self.query_rx.clone();
        let query_results_clone = self.query_results.clone();
        loop {
            // Fetch a query from the queue
            match query_rx_clone.recv() {
                Ok(query) => {
                    let result={
                        let search_library = self.search_library.read().unwrap();
                        search_library.search(&query)
                    };
                    query_results_clone.insert(query.id.clone(), result);
                }
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }

    pub fn process_each_query_across_all_threads_in_a_core(&self){
        let result_store: Arc<Mutex<QueryResult>>=Arc::new(Mutex::new(QueryResult::new("".to_string(),"".to_string(),vec![("".to_string(),0)],UNIX_EPOCH)));
        let current_item: Arc<(Mutex<Option<Query>>, Condvar)> = Arc::new((Mutex::new(None), Condvar::new()));
        let num_cores = core_affinity::get_core_ids().unwrap().len();
        let total_threads = num_cpus::get();
        let num_threads_per_core = if num_cores > 0 {
                    total_threads / num_cores
                } else {
                    1 // Default to 1 if no cores are found
                };
        let barrier = Arc::new(Barrier::new(num_threads_per_core));
        let search_library = self.search_library.clone();
        let query_results = self.query_results.clone();
        for _ in 0..num_threads_per_core {
            let result_store_clone = Arc::clone(&result_store);
            let current_item_clone = Arc::clone(&current_item);
            let search_library_clone = search_library.clone();
            let query_results_clone =  query_results.clone();
            let barrier_clone = Arc::clone(&barrier);
            thread::spawn(move || {
                loop {
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
                            let search_library = search_library_clone.read().unwrap();  // (2)
                            search_library.search(query)
                        };  // Lock is dropped here when search_library goes out of scope
                        let mut old_result = result_store_clone.lock().unwrap();
                        let updated_result = old_result.aggregate_result(result).clone();
                        std::mem::drop(old_result);
                        barrier_clone.wait(); // Wait for all threads to finish processing
                        query_results_clone.insert(updated_result.clone().query_id, updated_result)
                    }
                    // Signal that this thread has finished processing
                *query_item_opt = None;  // Reset the item
                cvar.notify_all(); // Notify other threads to check for the next item
                }
            });
        }
        //main thread which pulls one request from the query queue and passes it to each thread to process in parallel
        let query_rx_clone =  self.query_rx.clone();
        loop {
            
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
        }

    }

    pub fn is_alive(&self) -> bool {
        // Implement a health check, such as checking a heartbeat or status flag
        // This is just a stub implementation
        true
    }

    pub fn restart(&self) {
        // Logic to restart the processor if it's not functioning
        // This could involve restarting the thread or reinitializing resources
        let processor_clone = self.clone();
        
        thread::spawn(move || {
            processor_clone.process_queries();
        });
    }
}

pub struct Monitor {
    processors: Vec<Processor>,
}

impl Monitor {
    pub fn new(processors: Vec<Processor>) -> Self {
        Monitor { processors }
    }

    pub fn start_monitoring(&self) {
        let processors = self.processors.clone();
        thread::spawn(move || {
            loop {
                for processor in &processors {
                    // Perform health checks on each processor
                    if !processor.is_alive() {
                        // println!(
                        //     "Core: {}, Thread: {} has stopped working, restarting...",
                        //     processor.core_id, processor.thread_id
                        // );
                        // Restart the processor
                        processor.restart();
                    }
                }

                // Sleep for a period before the next check
                thread::sleep(Duration::from_secs(5));
            }
        });
    }
}