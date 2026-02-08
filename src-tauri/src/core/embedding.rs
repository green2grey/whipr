const DIMENSIONS: usize = 128;

pub fn embed_text(text: &str) -> Vec<f32> {
    let mut vector = vec![0.0_f32; DIMENSIONS];
    if text.is_empty() {
        return vector;
    }

    let mut token = String::with_capacity(24);
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            token.push(ch.to_ascii_lowercase());
        } else {
            push_token(&mut vector, &mut token);
        }
    }
    push_token(&mut vector, &mut token);

    normalize_vector(&mut vector);
    vector
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    if len == 0 {
        return 0.0;
    }
    let mut dot = 0.0_f32;
    let mut sum_a = 0.0_f32;
    let mut sum_b = 0.0_f32;
    for i in 0..len {
        let av = a[i];
        let bv = b[i];
        dot += av * bv;
        sum_a += av * av;
        sum_b += bv * bv;
    }
    let norm_a = sum_a.sqrt();
    let norm_b = sum_b.sqrt();
    let denom = norm_a * norm_b;
    if denom > 0.0 {
        dot / denom
    } else {
        0.0
    }
}

fn push_token(vector: &mut [f32], token: &mut String) {
    if token.len() < 2 {
        token.clear();
        return;
    }
    let weight = 1.0 + (token.len() as f32).ln();
    let idx = (fnv1a_hash(token.as_bytes()) % DIMENSIONS as u64) as usize;
    vector[idx] += weight;
    token.clear();
}

fn normalize_vector(vector: &mut [f32]) {
    let mut sum = 0.0_f32;
    for value in vector.iter() {
        sum += value * value;
    }
    let norm = sum.sqrt();
    if norm > 0.0 {
        for value in vector.iter_mut() {
            *value /= norm;
        }
    }
}

fn fnv1a_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x00000100000001b3_u64);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_text_is_deterministic() {
        let a = embed_text("Hello world, hello!");
        let b = embed_text("Hello world, hello!");
        assert_eq!(a.len(), DIMENSIONS);
        assert_eq!(b.len(), DIMENSIONS);
        assert!(cosine_similarity(&a, &b) > 0.99);
    }

    #[test]
    fn cosine_similarity_handles_non_normalized_vectors() {
        let a = [2.0_f32, 0.0];
        let b = [4.0_f32, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn embed_text_handles_empty() {
        let vector = embed_text("");
        assert_eq!(vector.len(), DIMENSIONS);
        assert!(vector.iter().all(|value| *value == 0.0));
    }
}
