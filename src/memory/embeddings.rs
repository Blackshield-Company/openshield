use std::collections::HashMap;

/// Fixed dimension for hash-based embedding vectors.
pub const EMBEDDING_DIM: usize = 1000;

/// A semantic embedding vector for text.
#[derive(Debug, Clone, PartialEq)]
pub struct Embedding(pub Vec<f32>);

impl Embedding {
    /// Create a new zero embedding.
    #[allow(dead_code)]
    pub fn zeros() -> Self {
        Self(vec![0.0; EMBEDDING_DIM])
    }

    /// Compute the L2 norm (magnitude) of the vector.
    pub fn norm(&self) -> f32 {
        self.0.iter().map(|v| v * v).sum::<f32>().sqrt()
    }

    /// Normalize the vector to unit length (in-place).
    pub fn normalize(&mut self) {
        let n = self.norm();
        if n > 1e-10 {
            for v in &mut self.0 {
                *v /= n;
            }
        }
    }

    /// Return a normalized copy of this embedding.
    #[allow(dead_code)]
    pub fn normalized(&self) -> Self {
        let mut copy = self.clone();
        copy.normalize();
        copy
    }
}

/// A simple set of English stop words to filter out.
const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being",
    "have", "has", "had", "do", "does", "did", "will", "would", "could", "should",
    "may", "might", "must", "shall", "can", "need", "dare", "ought", "used", "to",
    "of", "in", "for", "on", "with", "at", "by", "from", "as", "into", "through",
    "during", "before", "after", "above", "below", "between", "under", "again",
    "further", "then", "once", "here", "there", "when", "where", "why", "how",
    "all", "each", "few", "more", "most", "other", "some", "such", "no", "nor",
    "not", "only", "own", "same", "so", "than", "too", "very", "just", "and",
    "but", "if", "or", "because", "until", "while", "this", "that", "these", "those",
    "i", "me", "my", "myself", "we", "our", "ours", "ourselves", "you", "your",
    "yours", "yourself", "yourselves", "he", "him", "his", "himself", "she", "her",
    "hers", "herself", "it", "its", "itself", "they", "them", "their", "theirs",
    "themselves", "what", "which", "who", "whom", "am", "s", "t", "don", "didn",
    "doesn", "wasn", "weren", "won", "wouldn", "couldn", "shouldn", "isn", "aren",
    "hasn", "haven", "hadn", "does", "did", "will", "would", "could", "should",
];

/// Check if a word is a stop word.
fn is_stop_word(word: &str) -> bool {
    STOP_WORDS.contains(&word)
}

/// Tokenize text into lowercase alphanumeric tokens.
fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty() && s.len() > 1)
        .map(|s| s.to_string())
        .collect()
}

/// Hash a token to an index in [0, EMBEDDING_DIM).
fn hash_token(token: &str) -> usize {
    // Use a simple but well-distributed hash.
    let mut hash: u64 = 14695981039346656037; // FNV offset basis
    for byte in token.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(1099511628211); // FNV prime
    }
    (hash % EMBEDDING_DIM as u64) as usize
}

/// Compute term frequencies for a list of tokens, excluding stop words.
fn compute_term_frequencies(tokens: &[String]) -> HashMap<String, f32> {
    let mut tf = HashMap::new();
    let total = tokens.len() as f32;
    if total == 0.0 {
        return tf;
    }
    for token in tokens {
        if is_stop_word(token) {
            continue;
        }
        *tf.entry(token.clone()).or_insert(0.0) += 1.0;
    }
    // Normalize by total token count (including stop words, so rare terms get higher weight).
    for count in tf.values_mut() {
        *count /= total;
    }
    tf
}

/// Generate a semantic embedding vector from text.
///
/// Uses a hash-based vocabulary approach:
/// 1. Tokenize and lowercase the text.
/// 2. Filter out stop words.
/// 3. Compute term frequencies (TF).
/// 4. Hash each term to a fixed dimension index.
/// 5. Accumulate weighted frequencies into a sparse vector.
/// 6. Normalize the resulting vector.
///
/// This is deterministic, fast, and requires no external dependencies.
pub fn generate_embedding(text: &str) -> Vec<f32> {
    let tokens = tokenize(text);
    let tf = compute_term_frequencies(&tokens);

    let mut vec = vec![0.0f32; EMBEDDING_DIM];
    for (term, freq) in tf {
        let idx = hash_token(&term);
        // Weight by sqrt of frequency to dampen very frequent terms.
        vec[idx] += freq.sqrt();
    }

    // Normalize to unit vector for cosine similarity.
    let mut embedding = Embedding(vec);
    embedding.normalize();
    embedding.0
}

/// Compute cosine similarity between two vectors.
///
/// Both vectors are assumed to be normalized (unit length).
/// Returns a value in [-1.0, 1.0], where 1.0 means identical direction.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    if a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    // Clamp to [-1, 1] to avoid floating-point drift.
    dot.clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_basic() {
        let tokens = tokenize("Hello, world! This is a test.");
        assert_eq!(tokens, vec!["hello", "world", "this", "is", "test"]);
    }

    #[test]
    fn test_tokenize_empty() {
        let tokens = tokenize("");
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_hash_token_deterministic() {
        let h1 = hash_token("rust");
        let h2 = hash_token("rust");
        assert_eq!(h1, h2);
        assert!(h1 < EMBEDDING_DIM);
    }

    #[test]
    fn test_hash_token_distribution() {
        // Different words should generally hash to different indices.
        let h1 = hash_token("apple");
        let h2 = hash_token("banana");
        let h3 = hash_token("cherry");
        assert_ne!(h1, h2);
        assert_ne!(h2, h3);
    }

    #[test]
    fn test_compute_term_frequencies() {
        let tokens = vec![
            "rust".to_string(),
            "programming".to_string(),
            "rust".to_string(),
        ];
        let tf = compute_term_frequencies(&tokens);
        assert_eq!(tf.len(), 2);
        assert_eq!(tf.get("rust").copied().unwrap_or(0.0), 2.0 / 3.0);
        assert_eq!(tf.get("programming").copied().unwrap_or(0.0), 1.0 / 3.0);
    }

    #[test]
    fn test_stop_words_filtered() {
        let tokens = vec!["the".to_string(), "rust".to_string(), "a".to_string()];
        let tf = compute_term_frequencies(&tokens);
        assert_eq!(tf.len(), 1);
        assert!(tf.contains_key("rust"));
    }

    #[test]
    fn test_stop_words_filtered_with_is() {
        let tokens = vec!["the".to_string(), "rust".to_string(), "is".to_string()];
        let tf = compute_term_frequencies(&tokens);
        assert_eq!(tf.len(), 1);
        assert!(tf.contains_key("rust"));
    }

    #[test]
    fn test_generate_embedding_basic() {
        let emb = generate_embedding("rust programming");
        assert_eq!(emb.len(), EMBEDDING_DIM);
        // Should be normalized.
        let norm: f32 = emb.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5 || norm < 1e-10);
    }

    #[test]
    fn test_generate_embedding_empty() {
        let emb = generate_embedding("");
        assert_eq!(emb.len(), EMBEDDING_DIM);
        assert!(emb.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn test_generate_embedding_deterministic() {
        let e1 = generate_embedding("rust is great for systems programming");
        let e2 = generate_embedding("rust is great for systems programming");
        assert_eq!(e1, e2);
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let a = generate_embedding("rust programming language");
        let b = generate_embedding("rust programming language");
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_cosine_similarity_different() {
        let a = generate_embedding("rust programming");
        let b = generate_embedding("python scripting");
        let sim = cosine_similarity(&a, &b);
        // Should be low but non-negative.
        assert!(sim >= 0.0);
        assert!(sim < 0.9);
    }

    #[test]
    fn test_cosine_similarity_related() {
        let a = generate_embedding("rust programming language");
        let b = generate_embedding("rust coding and development");
        let sim = cosine_similarity(&a, &b);
        // Should be higher than completely unrelated.
        assert!(sim > 0.0);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 0.0).abs() < 1e-5);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![-1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - (-1.0)).abs() < 1e-5);
    }

    #[test]
    fn test_cosine_similarity_mismatched_lengths() {
        let a = vec![1.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn test_embedding_zeros() {
        let emb = Embedding::zeros();
        assert_eq!(emb.0.len(), EMBEDDING_DIM);
        assert!(emb.0.iter().all(|&v| v == 0.0));
        assert_eq!(emb.norm(), 0.0);
    }

    #[test]
    fn test_embedding_normalize() {
        let mut emb = Embedding(vec![3.0, 4.0, 0.0]);
        emb.normalize();
        assert!((emb.norm() - 1.0).abs() < 1e-5);
        assert!((emb.0[0] - 0.6).abs() < 1e-5);
        assert!((emb.0[1] - 0.8).abs() < 1e-5);
    }

    #[test]
    fn test_embedding_normalize_zero_vector() {
        let mut emb = Embedding::zeros();
        emb.normalize();
        assert!(emb.0.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn test_semantic_similarity_ranking() {
        let query = generate_embedding("rust programming");
        let docs = [
            "How to write rust code for systems programming",
            "Python scripting and automation",
            "Rust memory management and ownership",
            "Cooking recipes for beginners",
        ];

        let mut scored: Vec<(String, f32)> = docs
            .iter()
            .map(|text| {
                let emb = generate_embedding(text);
                let sim = cosine_similarity(&query, &emb);
                (text.to_string(), sim)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        assert!(
            scored[0].0.contains("rust") || scored[0].0.contains("Rust"),
            "Expected rust-related top result, got: {}",
            scored[0].0
        );

        let rust_scores: Vec<f32> = scored
            .iter()
            .filter(|(t, _)| t.contains("rust") || t.contains("Rust"))
            .map(|(_, s)| *s)
            .collect();
        let non_rust_scores: Vec<f32> = scored
            .iter()
            .filter(|(t, _)| !t.contains("rust") && !t.contains("Rust"))
            .map(|(_, s)| *s)
            .collect();
        let min_rust = rust_scores.iter().fold(f32::INFINITY, |a, &b| a.min(b));
        let max_non_rust = non_rust_scores.iter().fold(0.0f32, |a, &b| a.max(b));
        assert!(
            min_rust > max_non_rust,
            "Rust docs should score higher than non-rust docs"
        );
    }
}
