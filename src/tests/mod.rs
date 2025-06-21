#[cfg(test)]
mod tests {
    use crate::models::{Status, TestResult};

    #[test]
    fn test_new_result_with_uuid() {
        let result = TestResult::new("Test Case".to_string());

        // A UUID v4 is 36 characters long (including hyphens)
        assert_eq!(result.uuid.to_string().len(), 36);
        assert_eq!(result.name, "Test Case");
        assert_eq!(result.status, Status::Unknown);
    }

    #[test]
    fn test_start_stop_timing() {
        let mut result = TestResult::new("Test Case".to_string());

        // Start the test
        result.start();
        assert!(result.start.is_some());

        // Stop the test
        result.stop();
        assert!(result.stop.is_some());

        // Stop time should be greater than or equal to start time
        assert!(result.stop.unwrap() >= result.start.unwrap());
    }
}
