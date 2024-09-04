use std::sync::{Barrier, Condvar, Mutex};
use std::{sync::Arc, thread::ThreadId};
use std::time::Duration;
use std::thread;
use core_affinity::CoreId;
use search_engine::{Query, QueryQueue, QueryResult, QueryResults, SearchLibrary};
use lazy_static::lazy_static;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ThreadData;

lazy_static! {
    static ref PROCESSORS: Mutex<Vec<Arc<Processor>>> = Mutex::new(Vec::new());
}

#[derive(Clone)]
pub struct Processor{
    search_library:Arc<Mutex<SearchLibrary>>,
    core_id: CoreId,
    thread_id: ThreadId,
    query_queue: Arc<QueryQueue>,
    query_results: Arc<QueryResults>,
}


impl Processor{
    pub fn new(search_library: Arc<Mutex<SearchLibrary>>,core_id: CoreId, thread_id: ThreadId,query_queue: Arc<QueryQueue>,query_results: Arc<QueryResults>) -> Self{
        Processor {
            search_library,
            core_id,
            thread_id,
            query_queue,
            query_results
        }
    }
    pub fn store_in_global(self) -> Arc<Self> {
        let processor = Arc::new(self);
        PROCESSORS.lock().unwrap().push(Arc::clone(&processor));
        processor
    }
    pub fn process_queries(&self){
        let query_queue_clone = self.query_queue.clone();
        let query_results_clone = self.query_results.clone();
        loop {
            let search_library = self.search_library.lock().unwrap();
            // Fetch a query from the queue
            if let Some(query) = query_queue_clone.dequeue() {
                // Process the query using the search library
                let result = search_library.search(&query);

                // Update the query results
                query_results_clone.insert(query.id.clone(), result);

                // println!(
                //     "Core: {}, Thread: {}, Processed Query: {}",
                //     self.core_id, self.thread_id, query.id
                // );
            } else {
                // Sleep for a short duration before trying again if no queries are available
                std::thread::sleep(Duration::from_millis(100));
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
        let mut handles = vec![];
        let search_library = self.search_library.clone();
        let query_results = self.query_results.clone();
        for thread_id in 0..num_threads_per_core {
            let result_store_clone = Arc::clone(&result_store);
            let current_item_clone = Arc::clone(&current_item);
            let search_library_clone = search_library.clone();
            let query_results_clone =  query_results.clone();
            let barrier_clone = Arc::clone(&barrier);
            let handle = thread::spawn(move || {
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
                            let search_library = search_library_clone.lock().unwrap();  // (2)
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
            handles.push(handle);
        }
        let query_queue_clone = self.query_queue.clone();
        //main thread which pulls one request from the query queue and passes it to each thread to process in parallel
        loop {
            if let Some(query) = query_queue_clone.dequeue() {
                let (lock, cvar) = &*current_item;
                let mut current_item_lock = lock.lock().unwrap();
                *current_item_lock = Some(query);
                cvar.notify_all(); // Notify threads that an item is ready to be processed
    
                // Wait until all threads have finished processing
                while current_item_lock.is_some() {
                    current_item_lock = cvar.wait(current_item_lock).unwrap();
                }
            } 
        }
        // Join all threads
    
    // for handle in handles {
    //     match handle.join().unwrap() {
    //         Ok(_) => println!("Thread  completed successfully."),
    //         Err(e) => eprintln!("Thread  panicked: {:?}", e),
    //     }
    // }

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