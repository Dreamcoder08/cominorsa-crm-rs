use std::future::Future;

use thiserror::Error;

use crate::Lead;

#[derive(Debug, Error)]
#[error("No se pudo acceder a los datos: {0}")]
pub struct RepositoryError(pub String);

/// Persistence port. SQLite implements it today; a server database can replace it.
pub trait LeadRepository: Send + Sync {
    fn save(&self, lead: &Lead) -> impl Future<Output = Result<(), RepositoryError>> + Send;

    fn list(&self) -> impl Future<Output = Result<Vec<Lead>, RepositoryError>> + Send;
}
