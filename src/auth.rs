use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand_core::OsRng;


pub fn hash_password(password: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2.hash_password(password.as_bytes(), &salt).unwrap().to_string();
    password_hash
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let hash_trimmed = hash.trim();
    let parsed_hash = match PasswordHash::new(hash_trimmed) {
        Ok(h) => h,
        Err(e) => {
            println!("Error parsing password hash: {}", e);
            return false;
        }
    };
    Argon2::default().verify_password(password.as_bytes(), &parsed_hash).is_ok()
}

