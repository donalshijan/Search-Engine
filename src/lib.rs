use std::{collections::HashMap, vec};
use regex::Regex;
use scraper::{Html, Selector};
use serde_json::Value;

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
pub struct Query {
    pub id: String,
    pub query_string : String,
    pub tokens : Vec<String>
}

pub struct Result {
    pub query_id: String,
    pub query_string : String,
    pub documents: Vec<(String, f32)>
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
            query_string: query_string.to_string(),
            tokens: Vec::new(),
        }
    }
    
    pub fn tokenize_query(&mut self) {
        let re = Regex::new(r"\w+").unwrap(); // Matches words
        self.tokens = re.find_iter(&self.query_string)
            .map(|mat| mat.as_str().to_lowercase())
            .collect();
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

impl Result {
    // Constructor
    pub fn new(query_id: String, query_string: String) -> Self {
        Result {
            query_id,
            query_string,
            documents: Vec::new(), // Initialize with an empty vector of tuples
        }
    }
}


pub struct Searcher {
    pub index: HashMap<String, Vec<String>>,
}

impl Searcher {
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
    }
}
