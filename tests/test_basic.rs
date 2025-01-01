/// Tests for the Plenty interpreter. We will load in scripts of Plenty and execute them,
/// checking the output against the expected output.
use log::*;
use plenty::Stack;
use pretty_env_logger;
use rstest::{rstest, fixture};

#[fixture]
fn setup_logger() {
    let _ = pretty_env_logger::try_init();
}

#[rstest]
#[case("1 2 + .", vec!["[NumberI32(3)]"])]
#[case("3 4 + .", vec!["[NumberI32(7)]"])]
#[case("5 5 * .", vec!["[NumberI32(25)]"])]
#[case("10 2 - .", vec!["[NumberI32(8)]"])]
#[case("1 2 3 4 5 6 +", vec!["[NumberI32(1), NumberI32(2), NumberI32(3), NumberI32(4), NumberI32(11)]"])]
#[case("1 2 +\n3 +", vec!["[NumberI32(6)]"])]
fn test_programs(setup_logger: (), #[case] program: &str, #[case] expected_output: Vec<&str>) {
    let mut stack = Stack::new();
    let actual_output = stack.run_program(program).unwrap();
    assert_eq!(actual_output, expected_output);
}

// Let's test function creation
#[rstest]
#[case("` add + ~ :make-fn", "{\"add\": [Text(\"+\")]}")]
fn test_function_creation(setup_logger: (), #[case] program: &str, #[case] expected_output: &str) {
    let mut stack = Stack::new();
    let _actual_output = stack.run_program(program).unwrap();
    debug!("{:?}", stack.functions);
    let out = format!("{:?}", stack.functions);
    assert_eq!(out, expected_output);
}

// Let's test function calling
#[rstest]
// Define a new function, and then call it
#[case("` add + ~ :make-fn\n1 2 :add", vec!["[NumberI32(3)]"])]
// Important: no difference between newline or space
#[case("` add + ~ :make-fn 1 2 :add", vec!["[NumberI32(3)]"])]
fn test_function_calling(setup_logger: (), #[case] program: &str, #[case] expected_output: Vec<&str>) {
    let mut stack = Stack::new();
    let actual_output = stack.run_program(program).unwrap();
    debug!("after run program: {:?}", stack.repr());
    assert_eq!(actual_output, expected_output);
}
