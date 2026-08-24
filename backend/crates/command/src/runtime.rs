use serde_json::Value;

/// Transport-neutral classification for command failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandErrorClass {
    InvalidParams,
    InvalidRequest,
    TemporaryUnavailable,
    CapacityBusy,
    Internal,
}

/// An application command failure before a transport maps it to its wire
/// protocol error representation.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandError {
    pub class: CommandErrorClass,
    pub message: String,
    pub data: Option<Value>,
}

impl CommandError {
    pub fn new(class: CommandErrorClass, message: impl Into<String>) -> Self {
        Self {
            class,
            message: message.into(),
            data: None,
        }
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(CommandErrorClass::InvalidParams, message)
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(CommandErrorClass::InvalidRequest, message)
    }

    pub fn temporary_unavailable(message: impl Into<String>) -> Self {
        Self::new(CommandErrorClass::TemporaryUnavailable, message)
    }

    pub fn capacity_busy(message: impl Into<String>) -> Self {
        Self::new(CommandErrorClass::CapacityBusy, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(CommandErrorClass::Internal, message)
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn constructor_keeps_transport_neutral_payload() {
        let error = CommandError::invalid_params("target is required")
            .with_data(json!({"field": "target"}));

        assert_eq!(error.class, CommandErrorClass::InvalidParams);
        assert_eq!(error.message, "target is required");
        assert_eq!(error.data, Some(json!({"field": "target"})));
    }
}
