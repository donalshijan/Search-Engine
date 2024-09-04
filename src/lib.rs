use std::{collections::HashMap, vec};
use regex::Regex;
use scraper::{Html, Selector};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};
use lazy_static::lazy_static;


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
    pub query_processing_time: SystemTime,
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

impl Query {

    pub fn new(id: &str, query_string: &str) -> Self {
        Self {
            id: id.to_string(),
            query_string: String::from(query_string),
            tokens: Vec::new(),
            query_arrival_time: UNIX_EPOCH,
        }
    }
    
    pub fn tokenize_query(&mut self) -> Vec<String>{
        let re = Regex::new(r"\w+").unwrap(); // Matches words
        self.tokens = re.find_iter(&self.query_string)
            .map(|mat| mat.as_str().to_lowercase())
            .collect();
        self.tokens.clone()
    }

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
    pub fn new(query_id: String, query_string: String,documents:Vec<(String,i32)>,query_processing_time:SystemTime) -> Self {
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
        for document in new_result.documents {
            self.documents.push(document);
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
        for (doc_id, relevance) in &self.documents {
            // Fetch document from SearchLibrary (assuming you have a method for this)
            match SearchLibrary::get_document_by_id(doc_id) {
                Some(doc) => {
                    // Print the document's first 5 lines
                    let mut lines = match &doc.content {
                        DocumentFormat::PlainText(content) => content.lines(),
                        DocumentFormat::Html(content) => content.lines(), // Handle HTML appropriately
                        DocumentFormat::Json(content) => content.lines(), // Handle JSON appropriately
                    };

                    let mut line_count = 0;
                    while let Some(line) = lines.next() {
                        write!(f, "    Line {}: {}\n", line_count + 1, line)?;
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
}

impl SearchLibrary {
    pub fn new() -> Self {
        Self {
            index: HashMap::new(),
        }
    }

    pub fn create_index_from_existing_index(existing_index: HashMap<String, Vec<String>>) -> Self {
        Self {
            index: existing_index,
        }
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

    pub fn search(&self,query:& Query)-> QueryResult{
        let mut query = query.clone();
        let tokens:Vec<String>=query.tokenize_query();
        let mut result_docs = HashMap::new();
        for token in tokens {
            if let Some(docs) = self.index.get(&token) {
                for doc_id in docs {
                    *result_docs.entry(doc_id.clone()).or_insert(0) += 1;
                }
            }
        }

        // Sort documents by relevance (frequency of matching tokens)
        let mut sorted_docs: Vec<_> = result_docs.into_iter().collect();
        sorted_docs.sort_by(|a, b| b.1.cmp(&a.1)); // Sort by frequency descending
        let current_time = SystemTime::now();
        QueryResult::new(query.clone().id,query.clone().query_string,sorted_docs,current_time)
    }
}
