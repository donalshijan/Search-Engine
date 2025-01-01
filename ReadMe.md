# Project Overview
This is a search engine application, written in rust taking most advantage of it's memory safety and thread level and process level concurrency by leveraging it's high level and low level primitives for concurrency along with some atomics.
The program when run will first instantiate a Search engine instance, all throughout the code it is referred simply as engine, which will construct a Library, more appropriately a search library by using a bunch of documents specified in the documents folder at project root. The search library struct contains a field called index, as each document gets added to the search library, what actually happens is, first tokenize method gets called on the document to generate tokens for every word in the document , and these generated tokens become keys for the index and the document id of that document will be pushed to the corresponding vector mapped to those keys. So index is more like a `Hashmap<token=>Vector[document_id]>`
This Index is used to search for queries and produce results.

Search engine can operate on different modes or configurations so to say, which we do have to specify when we run the program as command line argument and it is used when engine instance is instantiated.

The searching algorithm is as follows, we tokenize the query string and for each token generated we will look up the document ids vector in the search library index using that token as key and construct a new hashmap for storing the query result  `HashMap<document_id=>relevance_score>`, all the document ids looked up from the index using the query token as key is inserted into the hashmap, if it is the first entry in the hashmap it gets a relevance score of 1, if not it gets updated to current score +1. After all the query tokens are processed in that manner, we will take the final result hashmap and take every key value entry in that map and construct a tuple of it and push it into a vector , and then we will sort that vector in descending order of second item in each tuple which is the relevance score. That basically generates a vector of tuples containing document ids and their respective relevance score(`Vec[(document_id,score)]`) from hightest to lowest order. This is the final result of the search query, it tells us which documents are most likely to be the ones containing relevant information about our query, and we can open and look up those documents using the document ids in the result. 
Every document's file name with the extension is used as the document's id.

Project also implements a monitoring service which will monitor each search processor and all the cores and threads it's assigned to, it can futher be extended to calculate memory usage and CPU usage for each core and thread but since it would rely on OS platform specific filesystem and APIs, so it has been left open for implementation by the user, but some skeletal code has been retained in comments.

Shutdown and thread terminations are handled gracefully.

## Architecture
The project's architecture involves of many components which handle their specific tasks.

### Server
The server component involves of two tcp listners set up on localhost to listen on port 8080 and 8081 for clients to make a search query request and for getting response for a search query respectively. When a client makes search request to port 8080 the server responds by returning a unique query_id for the request made by the client. The client is expected to keep polling on port 8081 with that query_id until it gets it's search response. Both these server listners run in an asynchronous runtime provided by tokio crate. New asynchronous tasks are created using tokio::spawn for every new request, thereby levaraging concurrency by efficiently managing scheduling of cpu between multiple coroutines running IO bound tasks involving client server communication over tcp.

### Search Library
Just as discussed above, it creates an index using all documents provided and also constructs a global store holding a mapping from each document id to it's respective document so that we can retrieve documents using their ids.

### Query Channel
Uses a multi producer multi consumer channel form crossbeam crate, which allows the queries to be pushed to channel when recieved by the server first and then pulled and processed by different processor components in parallel (or not) depending on the set mode of operation for the engine.

### Query Results 
As each query gets processed it's results are stored in a hashmap wrapped in `Arc<Mutex<HashMap<Query_id=>Result>>>`, the client is expected to keep polling with the query_id from the original search request response until the server endpoint at localhost 8081 can finally pull that respective result from the QueryResults hashmap when it is finally available.

### Processor
This is the component which processes the query and produces the results, the engine mode configuration we specify when setting up the engine, directly applies to the way this component's internal architecture will be set up to process queries in the overall system. Depending on the different modes of operation available for configuration, this component's architecture differs internally and the way it handles the processing of the queries both in terms of how many queries get processed in parallel, and how fast each can get processed, also differs.

Different engine mode configuration options, and how it affects the processing of the queries by the processor component is discussed here:

#### Modes
##### Single Core Single Thread
>In this mode there is only one single instance of processor (query processor) which is running inside a thread in one core of the cpu and processes all queries one after the other with no parallel processing or concurrency involved.

##### Multi Core Single Thread
>In this mode , based on how many cpu cores are on the machine running the program, that many processor instances are set up with each of those processor instances running in just one thread of one of the cores. All of the processor instances share the same search library instance wrapped in `Arc<RwLock<SearchLibrary>>` enabling all of them to access the search library's index field for simultaneous read access. RwLock allows multiple threads to have read access at a time, but only one thread can write. This way each of those processor instances can pull one query from the query channel and process in parallel. 

##### Multi Core Multi Thread (No Sharding)
>In this mode program tries to set up optimally maximum number of threads which can be run with significant gain in performance despite context switching overhead, running in each core out of the maximum number of cores available, each of those threads in a core have a processor instance, sharing referrence to the same search library which holds the index, in a thread safe manner using `Arc<RwLock<SearchLibrary>>`. Each of these processors pull a query from the query channel and process that query in parallel. Theoretically this should be able to process "total number of threads running processor instances across all cores" number of queries in parallel.

##### Multi Core Multi Thread With Sharding
>In this mode, program set's up number of cpu cores available number of processor instances, but each of these processor instances running in a core will utilize maximum number of threads that can run in that core in a way where each of those threads in that same core will process the same query in parallel but it only conducts the search in a subset of the index in search library, so we aggregate the results from each of those threads to produce the overall final result for that query. An example would make it more clear, if a machine running this program had 4 cpu cores and each of those cores allowed upto 4 threads in parallel as the optimal recommended number for that cpu. Then this program will pull one query to be processed by all four of the threads in one core, where each thread will search against a subset of keys in index , making the search space smaller for individual threads in hopes of gaining results sooner when multipe threads carry search in parallel over smaller search space, essentially implementing a logical sharding of the search space which is the whole index, across every thread in a core. This also means that in this mode, number of cpu cores available is the number of queries that will be processed in parallel, across all those cores at a time. So a machine with 4 cpu cores can process 4 queries in parallel in each of those cores, and every thread in each of those cores will be utilized to process that one query in parallel by dividing the search task into smaller search space for each of those threads to handle. It still uses the same search library referrence across all cores  wrapped in `Arc<RwLock<SearchLibrary>>`.It implements sophisticated coordination mechanism for threads in a core to process the same query in parallel using rust's synchronisation primitives namely Barrier,Mutex,Condvar and RwLock.

# How to start

>1. Install Rust and Cargo

>2. Compile and build project

Open terminal and run 

```
cargo build
```
This command will compile the main project binary (src/main.rs) as well as all *.rs files under src/bin as a separate binary.
The main project binary (src/main.rs) will get generated to target/debug/`<project_name>`.
Additional binaries (src/bin/*.rs) are generated to target/debug/`<binary_name>`.

## To simply run the project

Object code should be generated into `./target/debug/search_engine`

You will need to create a Documents folder and populate it with a bunch of files of type .txt or .html or .json , index will be created using these files. You need to pass the path to this directory as first argument when running the program.

You also need to decide and specify the mode in which to run the search engine

Following are the valid modes


```
single_core_single_thread
```
```
multi_core_single_thread
```
```
multi_core_multiple_threads_each_thread_searching_against_whole_index
```

```
multi_core_multiple_threads_each_thread_in_same_core_searching_against_single_sharded_subset_of_index
```
 One of these modes needs to be passed to the program as second argument.

 And lastly you also need to specify whether we you are running the program in testing or production mode by specifying either testing or production as the last command line argument when running the program.
## Usage Example
./target/debug/search_engine /path/to/documents_folder single_core_single_thread testing

## Test

I have also written a performance test which is included in the bin/test.rs which gets compiled to a separate binary when we run `cargo build`.
It can be used to run some performance tests on the project.

Object code for this test program should be generated to `./target/debug/test`

To run the test program 

```
cargo run --bin test
```
# Build and Run Test
I have also written a shell script which will build and run the project in all modes and run the same performance test program in each of those modes and calculate average processing time for processing x amount of queries in each mode and log the result to results.txt.

To run the script 

```
chmod +x build_and_run_tests.sh
./build_and_run_tests.sh
```

To simply build and run the program and interact with it in CLI,

```
chmod +x build_and_run_engine.sh
./build_and_run_engine.sh
```

