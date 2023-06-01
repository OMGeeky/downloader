
#[cfg(not(feature = "tracing"))]
use log::{debug, error, info, trace, warn};
#[cfg(feature = "tracing")]
pub use tracing::{debug, error, info, trace, warn};


pub use log::LevelFilter;