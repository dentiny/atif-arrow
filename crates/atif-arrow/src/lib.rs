mod batch;
mod convert;
mod error;
mod media;
mod parse;
mod schema;
mod validation;

pub use batch::TrajectoryBatchBuilder;
pub use convert::to_record_batch;
pub use error::{ConversionError, ParseError};
pub use media::{media_field, media_fields, trajectory_schema_with_media};
pub use schema::trajectory_schema;

pub use parse::{parse_trajectory, Agent, Message, ParsedTrajectory, Step, StepSource, Trajectory};
