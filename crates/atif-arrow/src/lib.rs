mod convert;
mod error;
mod parse;
mod reader;
mod schema;
mod validation;

pub use convert::to_record_batch;
pub use error::{ConversionError, ParseError, ReadError};
pub use reader::{InputFormat, TrajectoryReader};
pub use schema::trajectory_schema;

pub use parse::{parse_trajectory, Agent, Message, ParsedTrajectory, Step, StepSource, Trajectory};
