use argon2::password_hash::phc::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash};

pub fn generate_hash_from_password<T: AsRef<str>>(password: T) -> String {
    let salt = SaltString::generate();

    Argon2::default()
        .hash_password_with_salt(password.as_ref().as_bytes(), salt.as_bytes())
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
