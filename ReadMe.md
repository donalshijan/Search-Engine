# Project Overview
This is a search engine application, written in rust taking full advantage of it's memory safety and thread level and process level concurrency by leveraging it's high level primitives and low level primitives for concurrency along with some atomics.
Search engine constructs an index by using a bunch of documents specified in a directory later the index is used to search for queries and produce results.
Search engine can operate on different modes or configurations so to say.

Project also implements a monitoring service which will monitor each processor and calculate memory usage and CPU usage for each core and thread in the background.

Shutdown and thread terminations are handled gracefully.

## Modes
### Single Core Single Thread

>In this mode there is only one single instance of processor (query processor) which is running inside a thread in one core of the cpu and processes all queries .

### Multi Core Multi Thread (No Sharding)

>In this mode there are optimally maximum number of threads which can be run with significant gain in performance despite context switching overhead, running in maximum number of cores available, each of these threads have a processor instance , but not copying the index but rather using referance to the same index in a thread safe manner using `Arc<RwLock>`. 
Each of these processors pull a query from the query channel and process one query in parallel.

### Multi Core Multi Thread With Sharding

> In this mode there are optimally maximum number of threads which can be run with significant gain in performance despite context switching overhead, running in maximum number of cores available, each of these threads have a processor instance. Each of these processors running in the same core will pull from the query channel and process the same query, because each processor instance in same core is processing against a sharded subset of the original index. Results of each of these processors are aggregated into one final result only then do all processors move on to pull the next query from channel and start processing. It implements sophisticated coordination mechanism for threads to process the same query in parallel using rust's synchronisation primitives Barrier,Mutex,Condvar and RwLock.

# How to start

>1. Install Rust and Cargo

>2. Compile and build project

Open terminal and run 

```
cargo build
```

## To simply run the project

Object code should be generated into `./target/debug/search_engine`

You will need to create a Documents folder and populate it with a bunch of files of type .txt or .html or .json , index will be created using these files. You need to pass the path to this directory as first argument when running the program.

You also need to decide and specify the mode in which to run the search engine

Following are the valid mode


```
single_core_single_thread
```
```
multi_core_multiple_threads_each_thread_searching_against_whole_index
```

```
multi_core_multiple_threads_each_thread_in_same_core_searching_against_single_sharded_subset_of_index
```
 One of these modes needs to be passed to the program as second argument.
## Usage Example
./target/debug/search_engine /path/to/documents_folder single_core_single_thread

# Run test

I have also written a performance test which is included in the lib.rs which needs to be compiled to a separate test binary.

```
cargo build --bin main
```

I have also written a shell script which will run the project in all modes and run the same test in each of those modes and calculate average processing time for processing x amount of queries in each mode and log the result to result.txt 

To run the script 

```
chmod +x run_tests.sh
./run_tests.sh
```