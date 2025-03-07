
use std::{collections::HashMap,collections::HashSet,collections::BinaryHeap, vec};
use std::cmp::Reverse;
use regex::Regex;
use scraper::{Html, Selector};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use std::fmt;
use std::time::{Duration, SystemTime};
use lazy_static::lazy_static;


// Global store for storing A documents in the search library in a hashmap with document id used for look up
lazy_static! {
    static ref DOCUMENTS: Mutex<HashMap<String, Document>> = Mutex::new(HashMap::new());
}

#[derive(Debug, Clone)]
pub enum DocumentFormat {
    PlainText(String),
    Html(String),
    Json(String),
}

#[derive(Debug, Clone)]
pub struct Document {
    pub id: String,
    pub content: DocumentFormat,
}
#[derive(Debug, Clone)]
pub struct Query {
    pub id: String,
    pub query_string : String,
    pub tokens : Vec<String>,
    pub query_arrival_time: SystemTime,
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub query_id: String,
    pub query_string : String,
    pub documents: Vec<(String, i32)>,
    pub query_processing_time: Duration,
}

pub enum QueryChannelSenderMessage {
    Query(Query),
    Stop(String),
}


impl Document {
    pub fn new(id: &str, content: DocumentFormat) -> Self {
        Self {
            id: id.to_string(),
            content,
        }
    }
}

impl Document {
    pub fn tokenize(&self) -> Vec<String> {
        match &self.content {
            DocumentFormat::PlainText(text) => self.tokenize_plain_text(text),
            DocumentFormat::Html(html) => self.tokenize_html(html),
            DocumentFormat::Json(json) => self.tokenize_json(json),
        }
    }

    fn tokenize_plain_text(&self, text: &str) -> Vec<String> {
        text.split_whitespace()
            .map(|s| s.to_lowercase())
            .collect()
    }

    fn tokenize_html(&self, html: &str) -> Vec<String> {
        // Parse the HTML
        let document = Html::parse_document(html);
    
        // Create a selector to extract text from the document
        let selector = Selector::parse("body").unwrap(); // Adjust selector as needed
    
        // Extract text from the document
        let mut text = String::new();
        for node in document.select(&selector) {
            text.push_str(&node.text().collect::<Vec<_>>().join(" "));
        }
    
        // Tokenize the extracted text
        text.split_whitespace()
            .map(|s| s.to_lowercase())
            .collect()
    }

    fn tokenize_json(&self, json: &str) -> Vec<String> {
        // Parse the JSON string into a serde_json::Value
        let parsed: Value = match serde_json::from_str(json) {
            Ok(value) => value,
            Err(_) => return vec![], // Return an empty vector if parsing fails
        };

        // Use a recursive helper function to extract and tokenize text
        let mut tokens = Vec::new();
        self.extract_tokens(&parsed, &mut tokens);

        // Return the vector of tokens
        tokens
    }
        // Recursive helper function to extract tokens from a JSON Value
        fn extract_tokens(&self, value: &Value, tokens: &mut Vec<String>) {
            match value {
                // If the value is a string, add it to the tokens
                Value::String(s) => {
                    // Tokenize the string and add each token to the set
                    s.split_whitespace()
                        .map(|s| s.to_lowercase())
                        .for_each(|s| { tokens.push(s); });
                }
                // If the value is an object or array, recursively process its members
                Value::Object(map) => {
                    for (key, v) in map {
                        // Tokenize the key
                        key.split_whitespace()
                            .map(|s| s.to_lowercase())
                            .for_each(|s| { tokens.push(s); });
                        self.extract_tokens(v, tokens);
                    }
                }
                Value::Array(array) => {
                    for v in array {
                        self.extract_tokens(v, tokens);
                    }
                }
                // If the value is a number, boolean, or null, do nothing
                _ => {}
            }
        }
}

pub struct LRUCache {
    capacity: usize,
    cache: HashMap<String, Vec<(String, i32)>>, // (query_string -> results)
    order: VecDeque<String>, // Keeps track of the LRU order
}

impl LRUCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            cache: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn get(&mut self, query: &str) -> Option<Vec<(String, i32)>> {
        if let Some(results) = self.cache.get_mut(query) {
            // Move the query to the back to mark it as recently used
            self.order.retain(|q| q != query);
            self.order.push_back(query.to_string());
            return Some(results.clone());
        }
        None
    }

    pub fn put(&mut self, query: String, results: Vec<(String, i32)>) {
        if self.cache.contains_key(&query) {
            self.order.retain(|q| q != &query);
        } else if self.cache.len() >= self.capacity {
            // Evict the least recently used item
            if let Some(lru_query) = self.order.pop_front() {
                self.cache.remove(&lru_query);
            }
        }
        self.order.push_back(query.clone());
        self.cache.insert(query, results);
    }
}


lazy_static! {
    pub static ref LRU_CACHE: Mutex<LRUCache> = Mutex::new(LRUCache::new(10));
}

impl Query {

    pub fn new(id: &str, query_string: &str) -> Self {
        Self {
            id: id.to_string(),
            query_string: String::from(query_string),
            tokens: Vec::new(),
            query_arrival_time: std::time::SystemTime::now(),
        }
    }
    
    pub fn tokenize_query(&mut self) -> Vec<String>{
        let re = Regex::new(r"\w+").unwrap(); // Matches words
        self.tokens = re.find_iter(&self.query_string)
            .map(|mat| mat.as_str().to_lowercase())
            .collect();
        self.tokens.clone()
    }
    
    //This process_query method might be obsolete
    pub fn process_query(&self, index: &HashMap<String, Vec<String>>) -> Vec<String> {
        
        // Find relevant document IDs
        let mut result_docs = HashMap::new();
        for token in &self.tokens {
            if let Some(docs) = index.get(token) {
                for doc_id in docs {
                    *result_docs.entry(doc_id.clone()).or_insert(0) += 1;
                }
            }
        }

        // Sort documents by relevance (frequency of matching tokens)
        let mut sorted_docs: Vec<_> = result_docs.into_iter().collect();
        sorted_docs.sort_by(|a, b| b.1.cmp(&a.1)); // Sort by frequency descending

        sorted_docs.into_iter().map(|(doc_id, _)| doc_id).collect()
    }
}

impl QueryResult {
    // Constructor
    pub fn new(query_id: String, query_string: String,documents:Vec<(String,i32)>,query_processing_time:Duration) -> Self {
        QueryResult {
            query_id,
            query_string,
            documents,
            query_processing_time, 
        }
    }
    pub fn aggregate_result(&mut self,new_result:QueryResult)-> & mut QueryResult{
        // Check if self.query_id is the dummy value
        if self.query_id.is_empty() {
            // Copy the entire new_result to self
            self.query_id = new_result.query_id;
            self.query_string = new_result.query_string;
            self.documents = new_result.documents;
            self.query_processing_time = new_result.query_processing_time;
        } else if self.query_id != new_result.query_id {
            // If query_id doesn't match, return early
            return self;
        } else {
            // If query_id matches, aggregate the results
            // This was the previous approach which was incorrect as it made duplicate entries in the documents array when it should have updated the relevance score for existing ones
            // for document in new_result.documents {
            //     self.documents.push(document);
            // }
            for (doc_id, relevance) in new_result.documents {
                if let Some(existing_doc) = self
                    .documents
                    .iter_mut()
                    .find(|(existing_doc_id, _)| existing_doc_id == &doc_id)
                {
                    // Update the relevance score if the document already exists
                    existing_doc.1 += relevance;
                } else {
                    // Add the document if it does not already exist
                    self.documents.push((doc_id, relevance));
                }
            }
            // Sort documents by frequency in descending order
            self.documents.sort_by(|a, b| b.1.cmp(&a.1));
            self.query_processing_time = std::cmp::max(self.query_processing_time, new_result.query_processing_time);
        }
        
        self
    }
}

impl fmt::Display for QueryResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Format query result
        write!(f, "QueryResult:\n")?;
        write!(f, "  Query ID: {}\n", self.query_id)?;
        write!(f, "  Query String: {}\n", self.query_string)?;
        write!(f, "  Processing Time: {:?}\n", self.query_processing_time)?;

        // Fetch and print documents
        for (doc_id, relevance) in self.documents.iter().take(10) {
            // Fetch document from SearchLibrary (assuming you have a method for this)
            match SearchLibrary::get_document_by_id(doc_id) {
                Some(doc) => {
                    // Print the document's first 5 lines
                    let mut lines = match &doc.content {
                        DocumentFormat::PlainText(content) => content.lines(),
                        DocumentFormat::Html(content) => content.lines(), // Handle HTML appropriately
                        DocumentFormat::Json(content) => content.lines(), // Handle JSON appropriately
                    };
                    write!(f, "\n    Document ID(file name): {}\n", doc_id)?;
                    let mut line_count = 0;
                    while let Some(line) = lines.next() {
                        write!(f, "     {}\n", line)?;
                        line_count += 1;
                        if line_count >= 5 {
                            break;
                        }
                    }

                    // Print relevance score
                    write!(f, "    Relevance Score: {:.2}\n", relevance)?;
                }
                None => {
                    write!(f, "    Document with ID {} not found.\n", doc_id)?;
                }
            }
        }
        
        Ok(())
    }
}

//this struct might be obsolete
pub struct QueryQueue {
    queue: Arc<Mutex<VecDeque<Query>>>,
}

impl QueryQueue {
    // Create a new empty queue
    pub fn new() -> Self {
        QueryQueue {
            queue: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    // Add a new query to the queue
    pub fn enqueue(&self, query: Query) {
        let mut queue = self.queue.lock().unwrap();
        queue.push_back(query);
    }

    // Remove and return a query from the queue
    pub fn dequeue(&self) -> Option<Query> {
        let mut queue = self.queue.lock().unwrap();
        queue.pop_front()
    }

    // Check the current size of the queue
    pub fn size(&self) -> usize {
        let queue = self.queue.lock().unwrap();
        queue.len()
    }
}

pub struct QueryResults {
    results: Arc<Mutex<HashMap<String, QueryResult>>>,
}

impl QueryResults {
    // Create a new empty results queue
    pub fn new() -> Self {
        QueryResults {
            results: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // Add a new query result to the hashmap
    pub fn insert(&self, query_id: String, query_result: QueryResult) {
        let mut results = self.results.lock().unwrap();
        results.insert(query_id, query_result);
    }

    // Remove and return a query result from the hashmap
    pub fn remove(&self, query_id: &str) -> Option<QueryResult> {
        let mut results = self.results.lock().unwrap();
        results.remove(query_id)
    }

    // Check the current size of the hashmap
    pub fn size(&self) -> usize {
        let results = self.results.lock().unwrap();
        results.len()
    }

    // Retrieve a query result by query_id
    pub fn get_query_result(&self, query_id: &str) -> Option<QueryResult> {
        let results = self.results.lock().unwrap();
        results.get(query_id).cloned()
    }
}


pub struct SearchLibrary {
    pub index: HashMap<String, Vec<String>>,
    // Define stopword categories
    articles: HashSet<String>,
    prepositions_conjunctions: HashSet<String>,
    determiners_pronouns: HashSet<String>,
    auxiliary_verbs: HashSet<String>,
    common_adverbs: HashSet<String>,
}

impl SearchLibrary {
    pub fn new() -> Self {
        Self {
            index: HashMap::new(),
            articles: Self::init_articles(),
            prepositions_conjunctions: Self::init_prepositions_conjunctions(),
            determiners_pronouns: Self::init_determiners_pronouns(),
            auxiliary_verbs: Self::init_auxiliary_verbs(),
            common_adverbs: Self::init_common_adverbs(),
        }
    }

    pub fn create_index_from_existing_index(existing_index: HashMap<String, Vec<String>>) -> Self {
        Self {
            index: existing_index,
            articles: Self::init_articles(),
            prepositions_conjunctions: Self::init_prepositions_conjunctions(),
            determiners_pronouns: Self::init_determiners_pronouns(),
            auxiliary_verbs: Self::init_auxiliary_verbs(),
            common_adverbs: Self::init_common_adverbs(),
        }
    }

    fn init_articles() -> HashSet<String> {
        ["a", "an", "the"].iter().map(|s| s.to_string()).collect()
    }

    fn init_prepositions_conjunctions() -> HashSet<String> {
        [
            "in", "on", "at", "by", "with", "about", "against", "between", "into", "through",
            "during", "before", "after", "above", "below", "to", "from", "up", "down", "and",
            "or", "but", "as", "because", "since", "if", "though", "although", "unless",
            "while", "whereas", "whether", "so", "yet", "for", "nor",
            "like", "such", "despite", "amidst", "within", "without", "upon",
            "beyond", "among", "along", "via", "concerning", "towards", "underneath"
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    fn init_determiners_pronouns() -> HashSet<String> {
        [
            "the", "a", "an", "this", "that", "these", "those", "my", "your", "his", "her",
            "its", "our", "their", "whose", "some", "any", "each", "every", "either", "neither",
            "no", "none", "all", "both", "many", "most", "several", "few", "one", "two",
            "first", "second", "third", "it", "he", "she", "we", "they", "you",
            "those", "their", "his", "its", "who", "what", "whom",
            "which", "whichever", "whomever", "whosever", "anybody", "somebody", "everybody"
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    fn init_auxiliary_verbs() -> HashSet<String> {
        [
            "be", "am", "is", "are", "was", "were", "being", "been", "have", "has", "had",
            "do", "does", "did", "shall", "will", "should", "would", "may", "might", "must",
            "can", "could",
            "shall", "will", "does", "did",
            "ought", "dare", "need"
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    fn init_common_adverbs() -> HashSet<String> {
        [
            "very", "too", "quite", "rather", "almost", "so", "just", "even", "only", "also",
            "always", "never", "sometimes", "often", "usually", "seldom", "rarely", "ever",
            "however", "therefore", "thus", "more", "less",
            "often", "sometimes", "always", "never", "even", "still",
            "perhaps", "maybe", "surely", "truly", "undoubtedly"
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    pub fn add_document_to_index(&mut self, document: &Document) {
        let tokens = document.tokenize();
        for token in tokens {
            self.index
                .entry(token)
                .or_insert_with(Vec::new)
                .push(document.id.clone());
        }
        let mut documents = DOCUMENTS.lock().unwrap();
        documents.insert(document.id.clone(), document.clone());
    }
    pub fn get_document_by_id( doc_id: &str) -> Option<Document> {
        let documents = DOCUMENTS.lock().unwrap();
        documents.get(doc_id).cloned()
    }
    // Method to search the query in a subset of the search library index specified by shard_range if not specified then search is carried in the whole index
    pub fn search(&self, query: &Query, shard_range: Option<&[String]>) -> QueryResult {
        let mut query = query.clone();
        // Check the cache
        if let Some(cached_result) = LRU_CACHE.lock().unwrap().get(query.query_string.as_str()){
            println!("Cache Hit for query");

            let current_time = SystemTime::now();
            let duration_since_query_arrival = current_time
                .duration_since(query.query_arrival_time)
                .unwrap_or_else(|_| Duration::from_secs(0));
            return QueryResult::new(query.id.clone(), query.query_string.clone(), cached_result, duration_since_query_arrival);
        }
        let tokens: Vec<String> = query.tokenize_query();
        let mut result_docs = HashMap::new();
        let mut heap = BinaryHeap::new();
        
        let mut word_counts: HashMap<(&String, String), i32> = HashMap::new(); // (doc_id, token) -> count

        for token in tokens {
            // Determine whether to search the full index or a subset (shard)
            let search_docs = if let Some(keys_subset) = shard_range {
                if keys_subset.contains(&token) {
                    self.index.get(&token)
                } else {
                    None
                }
            } else {
                self.index.get(&token)
            };
            
            if let Some(docs) = search_docs {
                for doc_id in docs {
                    let entry = result_docs.entry(doc_id.clone()).or_insert(0);
                    let count = word_counts.entry((doc_id, token.to_owned())).or_insert(0);

                    if self.articles.contains(token.as_str()) {
                        // Add 1 only once per document
                        if *count == 0 {
                            *entry += 1;
                        }
                    } else if self.prepositions_conjunctions.contains(token.as_str()) {
                        // Add 1 up to 2 times
                        if *count < 2 {
                            *entry += 1;
                        }
                    } else if self.determiners_pronouns.contains(token.as_str()){
                        // Add 2 up to 2 times
                        if *count < 2 {
                            *entry += 2;
                        }
                    } 
                    else if self.auxiliary_verbs.contains(token.as_str()){
                        // Add 2 up to 1 times
                        if *count < 1 {
                            *entry += 2;
                        }
                    }
                    else if self.common_adverbs.contains(token.as_str()){
                        // Add 2 up to 2 times
                        if *count < 2 {
                            *entry += 2;
                        }
                    }  else {
                        // Default full weight
                        if *count < 3{
                            *entry += 6;
                        }
                    }
                    
                    *count += 1; // Track occurrences
                }
            }
        }

        // Step 2: Push results into a max-heap
        for (doc_id, count) in result_docs {
            heap.push(Reverse((count, doc_id))); // Reverse to make it a min-heap
        }

        // Step 3: Extract results in sorted order
        // Sort documents by relevance (frequency of matching tokens)
        let sorted_docs: Vec<_> = heap.into_sorted_vec()
        .into_iter()
        .map(|Reverse((count, doc_id))| (doc_id, count)) // Unwrapping Reverse
        .collect();
    
        let current_time = SystemTime::now();
        // println!("current time {:?}", current_time);
        let duration_since_query_arrival = current_time
            .duration_since(query.query_arrival_time)
            .unwrap_or_else(|_| Duration::from_secs(0));
        // Store result in cache
        LRU_CACHE.lock().unwrap().put(query.query_string.clone(), sorted_docs.clone());
        QueryResult::new(query.id.clone(), query.query_string.clone(), sorted_docs, duration_since_query_arrival)
    }
}

