---
name: testing
description: Testing strategies and best practices
triggers:
  - test
  - testing
  - unittest
  - integration test
  - mock
  - fixture
  - tdd
  - coverage
tags:
  - quality
  - testing
---

# Testing Best Practices

## Unit Tests
- One assertion per test (ideally)
- Test behavior, not implementation
- Name tests descriptively: `test_foo_returns_error_when_bar_is_negative`
- Use `#[should_panic]` for error paths
- Mock external dependencies, not internal logic

## Integration Tests
- Test the full stack: API → service → database
- Use test databases or in-memory alternatives
- Clean up after each test (transactions, temp files)
- Parallelize where possible

## Test Data
- Use factories, not fixtures
- Faker libraries for realistic data
- Edge cases: empty, null, max values, unicode, special chars
- Property-based testing with `proptest` or `quickcheck`

## Coverage
- Aim for 80%+ coverage on critical paths
- 100% coverage is a vanity metric — focus on meaningful tests
- Use `cargo tarpaulin` for Rust coverage
- Don't test getters/setters unless they have logic

## Rust-Specific
- `tokio::test` for async tests
- `tempfile` crate for temp files
- `mockall` for mocking traits
- `assert_cmd` for CLI testing
- `pretty_assertions` for better diff output
