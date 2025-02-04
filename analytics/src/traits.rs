/// A trait for types that can be constructed from environment variables.
///
/// Types implementing this trait can be instantiated by reading their configuration
/// from environment variables. Implementors should provide sensible defaults and
/// handle missing or invalid environment variables gracefully.
///
/// # Implementation Guidelines
/// - Use appropriate environment variable naming conventions (e.g., UPPERCASE_WITH_UNDERSCORES)
/// - Provide clear logging/warnings when falling back to default values
/// - Handle invalid values robustly by falling back to defaults
/// - Document all environment variables used by the implementation
///
/// # Examples
/// ```
/// use std::env;
/// use tracing::warn;
///
/// struct DatabaseConfig {
///     host: String,
///     port: u16,
/// }
///
/// impl FromEnv for DatabaseConfig {
///     fn from_env() -> Self {
///         let default_host = "localhost".to_string();
///         let default_port = 5432;
///
///         Self {
///             host: env::var("DB_HOST").unwrap_or_else(|_| {
///                 warn!("DB_HOST not set, defaulting to {}", default_host);
///                 default_host
///             }),
///             port: env::var("DB_PORT")
///                 .map(|p| p.parse().unwrap_or(default_port))
///                 .unwrap_or_else(|_| {
///                     warn!("DB_PORT not set, defaulting to {}", default_port);
///                     default_port
///                 }),
///         }
///     }
/// }
/// ```
pub trait FromEnv {
    /// Creates a new instance of the type from environment variables.
    ///
    /// This method should read any required configuration from environment
    /// variables and construct a new instance of the implementing type.
    /// It should handle missing or invalid environment variables gracefully,
    /// typically by falling back to default values and logging warnings.
    ///
    /// # Returns
    /// A new instance of the implementing type, configured from
    /// environment variables where available and using defaults where necessary.
    fn from_env() -> Self
    where
        Self: Sized;
}
