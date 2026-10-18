//! Why the engine can't give a plan or a download: one enum for the crate. Its message is what the
//! editor shows (a `JsError`'s `message`, or a plan's `error`), so a variant with a cause says it
//! too.

/// The crate's result.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Why a topology gives no plan or no download.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The topology isn't the editor's JSON.
    #[error("the topology is malformed: {0}")]
    Malformed(#[source] serde_json::Error),
    /// The drawing doesn't compile.
    #[error(transparent)]
    Compile(#[from] isothrum_compiler::Error),
    /// Isoloom finds problems in the spec: nothing is generated until they're fixed.
    #[error("fix the spec first: {}", .0.join("; "))]
    Problems(Vec<String>),
    /// The project generates nothing for that target.
    #[error("this project has no files for `{0}`")]
    NoFiles(String),
    /// The zip couldn't be written.
    #[error("writing the zip")]
    Zip(#[from] zip::result::ZipError),
    /// A file couldn't be added to the zip.
    #[error("writing the zip")]
    Io(#[from] std::io::Error),
}
