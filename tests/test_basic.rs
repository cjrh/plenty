/// Tests for the Plenty interpreter. We will load in scripts of Plenty and execute them,
/// checking the output against the expected output.
use plenty::Stack;
use rstest::rstest;

#[rstest]
#[case("1 2 + .", vec!["[NumberI32(3)]"])]
#[case("3 4 + .", vec!["[NumberI32(7)]"])]
#[case("5 5 * .", vec!["[NumberI32(25)]"])]
#[case("10 2 - .", vec!["[NumberI32(8)]"])]
#[case("1 2 3 4 5 6 +", vec!["[NumberI32(1), NumberI32(2), NumberI32(3), NumberI32(4), NumberI32(11)]"])]
#[case("1 2 +\n3 +", vec!["[NumberI32(6)]"])]
fn test_programs(#[case] program: &str, #[case] expected_output: Vec<&str>) {
    let mut stack = Stack::new();
    let actual_output = stack.run_program(program).unwrap();
    assert_eq!(actual_output, expected_output);
}
