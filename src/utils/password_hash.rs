use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash};
use rand::Rng;
use rand::rngs::OsRng;

pub fn generate_random_password() -> String {
    let mut rng = rand::rng();
    let mut password = String::new();
    password.reserve(15);

    for _ in 0..15 {
        let random_char: u8 = rng.random_range(35..=122);
        password.push(random_char as char);
    }

    password
}

pub fn generate_hash_from_password<T: AsRef<str>>(password: T) -> String {
    let mut rng = OsRng;
    let salt = SaltString::try_from_rng(&mut rng).expect("Failed to generate salt for password");

    Argon2::default()
        .hash_password(password.as_ref().as_bytes(), &salt)
        .expect("Failed to hash password")
        .to_string()
}

pub fn validate_credentials<T: AsRef<str>, E: AsRef<str>>(
    password: T,
    expected_password_hash: E,
) -> Result<bool, password_hash::Error> {
    let expected_password_hash = PasswordHash::new(expected_password_hash.as_ref())?;

    Ok(Argon2::default()
        .verify_password(password.as_ref().as_bytes(), &expected_password_hash)
        .is_ok())
}
