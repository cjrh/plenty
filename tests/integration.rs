//! Every integration test, as modules of one test binary (issue #139).
//!
//! Cargo builds each test target as a separate binary and runs those binaries
//! one after another, so slow tests in different files could not overlap. As
//! modules of one binary, all tests share the harness's thread pool and the
//! binary is linked once. `Cargo.toml` sets `autotests = false`, so a new
//! `tests/test_*.rs` file runs only once it is listed here;
//! `every_test_file_is_listed` fails until then. Select one file's tests with
//! a module filter: `cargo test --test integration test_std_url::`.
//!
//! Rust treats a `#[path]` module like a `mod.rs` file, so the `mod support;`
//! in each file still resolves to `tests/support/mod.rs`. Each file keeps its
//! own copy of those helpers, as when every file was a separate crate.
#![allow(clippy::duplicate_mod)]

#[path = "test_pipelines.rs"]
mod test_pipelines;

#[path = "test_clist.rs"]
mod test_clist;

#[path = "test_std_dataframe.rs"]
mod test_std_dataframe;
#[path = "test_std_linalg.rs"]
mod test_std_linalg;

#[path = "test_aliases.rs"]
mod test_aliases;
#[path = "test_allocation.rs"]
mod test_allocation;
#[path = "test_allocation_construction.rs"]
mod test_allocation_construction;
#[path = "test_allocation_default.rs"]
mod test_allocation_default;
#[path = "test_allocation_markers.rs"]
mod test_allocation_markers;
#[path = "test_anonymous_functions.rs"]
mod test_anonymous_functions;
#[path = "test_anonymous_sums.rs"]
mod test_anonymous_sums;
#[path = "test_aot.rs"]
mod test_aot;
#[path = "test_augmented_assignment.rs"]
mod test_augmented_assignment;
#[path = "test_binary_io.rs"]
mod test_binary_io;
#[path = "test_bitwise.rs"]
mod test_bitwise;
#[path = "test_book_highlighting.rs"]
mod test_book_highlighting;
#[path = "test_boolean_lowering.rs"]
mod test_boolean_lowering;
#[path = "test_borrowed_iteration.rs"]
mod test_borrowed_iteration;
#[path = "test_borrowed_list_rows.rs"]
mod test_borrowed_list_rows;
#[path = "test_borrowed_match.rs"]
mod test_borrowed_match;
#[path = "test_borrowing.rs"]
mod test_borrowing;
#[path = "test_boxed_callables.rs"]
mod test_boxed_callables;
#[path = "test_boxes.rs"]
mod test_boxes;
#[path = "test_byte_literals.rs"]
mod test_byte_literals;
#[path = "test_byte_sources.rs"]
mod test_byte_sources;
#[path = "test_call_borrowing.rs"]
mod test_call_borrowing;
#[path = "test_call_substitution.rs"]
mod test_call_substitution;
#[path = "test_callable_bounds.rs"]
mod test_callable_bounds;
#[path = "test_callable_diagnostics.rs"]
mod test_callable_diagnostics;
#[path = "test_callable_integration.rs"]
mod test_callable_integration;
#[path = "test_callable_tail_calls.rs"]
mod test_callable_tail_calls;
#[path = "test_callable_values.rs"]
mod test_callable_values;
#[path = "test_callback_registry.rs"]
mod test_callback_registry;
#[path = "test_cancel_on_exit.rs"]
mod test_cancel_on_exit;
#[path = "test_channels.rs"]
mod test_channels;
#[path = "test_checked_add_sub.rs"]
mod test_checked_add_sub;
#[path = "test_class_allocation.rs"]
mod test_class_allocation;
#[path = "test_classes.rs"]
mod test_classes;
#[path = "test_cli.rs"]
mod test_cli;
#[path = "test_closure_captures.rs"]
mod test_closure_captures;
#[path = "test_closure_expressions.rs"]
mod test_closure_expressions;
#[path = "test_closure_factories.rs"]
mod test_closure_factories;
#[path = "test_closure_generics.rs"]
mod test_closure_generics;
#[path = "test_closure_safety.rs"]
mod test_closure_safety;
#[path = "test_collection_bulk.rs"]
mod test_collection_bulk;
#[path = "test_collection_mutations.rs"]
mod test_collection_mutations;
#[path = "test_collections.rs"]
mod test_collections;
#[path = "test_compile_cache.rs"]
mod test_compile_cache;
#[path = "test_compile_scaling.rs"]
mod test_compile_scaling;
#[path = "test_compiler_parallelism.rs"]
mod test_compiler_parallelism;
#[path = "test_concurrency_control.rs"]
mod test_concurrency_control;
#[path = "test_concurrency_modules.rs"]
mod test_concurrency_modules;
#[path = "test_constant_casts.rs"]
mod test_constant_casts;
#[path = "test_constant_guards.rs"]
mod test_constant_guards;
#[path = "test_consuming_methods.rs"]
mod test_consuming_methods;
#[path = "test_consuming_threads.rs"]
mod test_consuming_threads;
#[path = "test_container_conveniences.rs"]
mod test_container_conveniences;
mod test_context_reporting_bounds;
#[path = "test_contexts.rs"]
mod test_contexts;
#[path = "test_conversion_errors.rs"]
mod test_conversion_errors;
#[path = "test_copy_classes.rs"]
mod test_copy_classes;
#[path = "test_copyable_tuple_mutation.rs"]
mod test_copyable_tuple_mutation;
#[path = "test_data_callable_bounds.rs"]
mod test_data_callable_bounds;
#[path = "test_data_structure_patterns.rs"]
mod test_data_structure_patterns;
#[path = "test_dense_match.rs"]
mod test_dense_match;
#[path = "test_dictionary_lookup.rs"]
mod test_dictionary_lookup;
#[path = "test_dictionary_removal.rs"]
mod test_dictionary_removal;
#[path = "test_dictionary_snapshots.rs"]
mod test_dictionary_snapshots;
#[path = "test_dictionary_views.rs"]
mod test_dictionary_views;
#[path = "test_dump.rs"]
mod test_dump;
#[path = "test_element_references.rs"]
mod test_element_references;
#[path = "test_empty_release.rs"]
mod test_empty_release;
mod test_entry_report_format;
#[path = "test_entrypoint.rs"]
mod test_entrypoint;
#[path = "test_enum_allocation.rs"]
mod test_enum_allocation;
#[path = "test_enum_methods.rs"]
mod test_enum_methods;
#[path = "test_error_composition.rs"]
mod test_error_composition;
#[path = "test_error_context.rs"]
mod test_error_context;
#[path = "test_examples.rs"]
mod test_examples;
#[path = "test_executable_size.rs"]
mod test_executable_size;
#[path = "test_executors.rs"]
mod test_executors;
#[path = "test_exports.rs"]
mod test_exports;
#[path = "test_failure.rs"]
mod test_failure;
#[path = "test_fallible_collect.rs"]
mod test_fallible_collect;
#[path = "test_fallible_copy.rs"]
mod test_fallible_copy;
#[path = "test_fallible_displays.rs"]
mod test_fallible_displays;
#[path = "test_fallible_format.rs"]
mod test_fallible_format;
#[path = "test_fallible_text.rs"]
mod test_fallible_text;
#[path = "test_ffi.rs"]
mod test_ffi;
#[path = "test_field_indexing.rs"]
mod test_field_indexing;
#[path = "test_field_moves.rs"]
mod test_field_moves;
#[path = "test_field_replacement.rs"]
mod test_field_replacement;
#[path = "test_file_io.rs"]
mod test_file_io;
#[path = "test_file_mode.rs"]
mod test_file_mode;
#[path = "test_file_streams.rs"]
mod test_file_streams;
#[path = "test_filesystem.rs"]
mod test_filesystem;
#[path = "test_flat_classes.rs"]
mod test_flat_classes;
#[path = "test_float_constants.rs"]
mod test_float_constants;
#[path = "test_float_left_constants.rs"]
mod test_float_left_constants;
#[path = "test_float_negation.rs"]
mod test_float_negation;
#[path = "test_float_rounding.rs"]
mod test_float_rounding;
#[path = "test_fs_helpers.rs"]
mod test_fs_helpers;
#[path = "test_fstrings.rs"]
mod test_fstrings;
#[path = "test_fuzz.rs"]
mod test_fuzz;
#[path = "test_generator_allocation.rs"]
mod test_generator_allocation;
#[path = "test_generators.rs"]
mod test_generators;
#[path = "test_generic_data.rs"]
mod test_generic_data;
#[path = "test_generic_data_safety.rs"]
mod test_generic_data_safety;
#[path = "test_generic_inference.rs"]
mod test_generic_inference;
#[path = "test_generic_methods.rs"]
mod test_generic_methods;
#[path = "test_generic_unit.rs"]
mod test_generic_unit;
#[path = "test_generics.rs"]
mod test_generics;
#[path = "test_heap_drop_order.rs"]
mod test_heap_drop_order;
#[path = "test_indirect_ffi.rs"]
mod test_indirect_ffi;
#[path = "test_inline_constructors.rs"]
mod test_inline_constructors;
#[path = "test_inline_copies.rs"]
mod test_inline_copies;
#[path = "test_inline_generators.rs"]
mod test_inline_generators;
#[path = "test_inline_ranges.rs"]
mod test_inline_ranges;
#[path = "test_iteration_conveniences.rs"]
mod test_iteration_conveniences;
#[path = "test_key_sorting.rs"]
mod test_key_sorting;
#[path = "test_large_functions.rs"]
mod test_large_functions;
#[path = "test_last_use_moves.rs"]
mod test_last_use_moves;
#[path = "test_library_metadata.rs"]
mod test_library_metadata;
#[path = "test_library_publication.rs"]
mod test_library_publication;
#[path = "test_list_element_refs.rs"]
mod test_list_element_refs;
#[path = "test_list_fast_paths.rs"]
mod test_list_fast_paths;
#[path = "test_list_lookup.rs"]
mod test_list_lookup;
#[path = "test_list_ordering.rs"]
mod test_list_ordering;
#[path = "test_list_queries.rs"]
mod test_list_queries;
#[path = "test_list_removal.rs"]
mod test_list_removal;
#[path = "test_loop_counter_ranges.rs"]
mod test_loop_counter_ranges;
#[path = "test_loops.rs"]
mod test_loops;
#[path = "test_lsp.rs"]
mod test_lsp;
#[path = "test_match_values.rs"]
mod test_match_values;
#[path = "test_memory_order.rs"]
mod test_memory_order;
#[path = "test_modern.rs"]
mod test_modern;
#[path = "test_module_constants.rs"]
mod test_module_constants;
#[path = "test_modules.rs"]
mod test_modules;
#[path = "test_multiple_assignment.rs"]
mod test_multiple_assignment;
#[path = "test_named_arguments.rs"]
mod test_named_arguments;
#[path = "test_named_function_borrows.rs"]
mod test_named_function_borrows;
#[path = "test_nested_assignment.rs"]
mod test_nested_assignment;
#[path = "test_nested_closures.rs"]
mod test_nested_closures;
#[path = "test_nested_indexing.rs"]
mod test_nested_indexing;
#[path = "test_nesting.rs"]
mod test_nesting;
#[path = "test_newtype_classes.rs"]
mod test_newtype_classes;
#[path = "test_numeric_builtins.rs"]
mod test_numeric_builtins;
#[path = "test_numeric_casts.rs"]
mod test_numeric_casts;
#[path = "test_numeric_prelude.rs"]
mod test_numeric_prelude;
#[path = "test_numeric_text.rs"]
mod test_numeric_text;
#[path = "test_once_callable_bounds.rs"]
mod test_once_callable_bounds;
#[path = "test_once_closure_borrows.rs"]
mod test_once_closure_borrows;
#[path = "test_once_closure_safety.rs"]
mod test_once_closure_safety;
#[path = "test_once_closure_storage.rs"]
mod test_once_closure_storage;
#[path = "test_once_closures.rs"]
mod test_once_closures;
#[path = "test_operator_overloading.rs"]
mod test_operator_overloading;
#[path = "test_optional_references.rs"]
mod test_optional_references;
#[path = "test_ownership_runtime.rs"]
mod test_ownership_runtime;
#[path = "test_par_pipelines.rs"]
mod test_par_pipelines;
#[path = "test_parallel.rs"]
mod test_parallel;
#[path = "test_parameterized_protocols.rs"]
mod test_parameterized_protocols;
#[path = "test_patterns.rs"]
mod test_patterns;
#[path = "test_practical_io.rs"]
mod test_practical_io;
#[path = "test_profile.rs"]
mod test_profile;
#[path = "test_project.rs"]
mod test_project;
#[path = "test_propagation.rs"]
mod test_propagation;
#[path = "test_protocol_defaults.rs"]
mod test_protocol_defaults;
#[path = "test_protocols.rs"]
mod test_protocols;
#[path = "test_python_habit_diagnostics.rs"]
mod test_python_habit_diagnostics;
#[path = "test_query_methods.rs"]
mod test_query_methods;
#[path = "test_range_loops.rs"]
mod test_range_loops;
#[path = "test_receiver_arguments.rs"]
mod test_receiver_arguments;
#[path = "test_receiving_joins.rs"]
mod test_receiving_joins;
#[path = "test_recursive_data.rs"]
mod test_recursive_data;
#[path = "test_recursive_tail_errors.rs"]
mod test_recursive_tail_errors;
#[path = "test_reference_rebinding.rs"]
mod test_reference_rebinding;
#[path = "test_reference_tail_calls.rs"]
mod test_reference_tail_calls;
#[path = "test_release_stores.rs"]
mod test_release_stores;
#[path = "test_result_handling.rs"]
mod test_result_handling;
#[path = "test_retained_job_errors.rs"]
mod test_retained_job_errors;
#[path = "test_returned_references.rs"]
mod test_returned_references;
#[path = "test_returns.rs"]
mod test_returns;
#[path = "test_runtime_interface_cli.rs"]
mod test_runtime_interface_cli;
#[path = "test_runtime_libraries.rs"]
mod test_runtime_libraries;
#[path = "test_runtime_packaging.rs"]
mod test_runtime_packaging;
#[path = "test_runtime_panics.rs"]
mod test_runtime_panics;
#[path = "test_set_operations.rs"]
mod test_set_operations;
#[path = "test_set_removal.rs"]
mod test_set_removal;
#[path = "test_shared.rs"]
mod test_shared;
#[path = "test_shared_exits.rs"]
mod test_shared_exits;
#[path = "test_short_circuit_branches.rs"]
mod test_short_circuit_branches;
#[path = "test_slicing.rs"]
mod test_slicing;
#[path = "test_small_conveniences.rs"]
mod test_small_conveniences;
#[path = "test_stack_overflow.rs"]
mod test_stack_overflow;
#[path = "test_std_actor.rs"]
mod test_std_actor;
#[path = "test_std_actor_net.rs"]
mod test_std_actor_net;
#[path = "test_std_actor_tls.rs"]
mod test_std_actor_tls;
#[path = "test_std_archive.rs"]
mod test_std_archive;
#[path = "test_std_atomic.rs"]
mod test_std_atomic;
#[path = "test_std_bytes.rs"]
mod test_std_bytes;
#[path = "test_std_cache_lru.rs"]
mod test_std_cache_lru;
#[path = "test_std_cli_progress.rs"]
mod test_std_cli_progress;
#[path = "test_std_collections.rs"]
mod test_std_collections;
#[path = "test_std_complex.rs"]
mod test_std_complex;
#[path = "test_std_compress.rs"]
mod test_std_compress;
#[path = "test_std_crypto.rs"]
mod test_std_crypto;
#[path = "test_std_crypto_advanced.rs"]
mod test_std_crypto_advanced;
#[path = "test_std_crypto_blake3.rs"]
mod test_std_crypto_blake3;
#[path = "test_std_csv.rs"]
mod test_std_csv;
#[path = "test_std_diff.rs"]
mod test_std_diff;
#[path = "test_std_encoding.rs"]
mod test_std_encoding;
#[path = "test_std_env_process.rs"]
mod test_std_env_process;
#[path = "test_std_fmt.rs"]
mod test_std_fmt;
#[path = "test_std_fs_dirs.rs"]
mod test_std_fs_dirs;
#[path = "test_std_fs_watch.rs"]
mod test_std_fs_watch;
#[path = "test_std_graph.rs"]
mod test_std_graph;
#[path = "test_std_http.rs"]
mod test_std_http;
#[path = "test_std_http_client.rs"]
mod test_std_http_client;
#[path = "test_std_http_files.rs"]
mod test_std_http_files;
#[path = "test_std_http_helpers.rs"]
mod test_std_http_helpers;
#[path = "test_std_http_https.rs"]
mod test_std_http_https;
#[path = "test_std_http_server.rs"]
mod test_std_http_server;
#[path = "test_std_http_server_proxy.rs"]
mod test_std_http_server_proxy;
#[path = "test_std_http_server_qualification.rs"]
mod test_std_http_server_qualification;
#[path = "test_std_http_shared_client.rs"]
mod test_std_http_shared_client;
#[path = "test_std_http_sse.rs"]
mod test_std_http_sse;
#[path = "test_std_http_websocket.rs"]
mod test_std_http_websocket;
#[path = "test_std_io.rs"]
mod test_std_io;
#[path = "test_std_json.rs"]
mod test_std_json;
#[path = "test_std_kv.rs"]
mod test_std_kv;
#[path = "test_std_log.rs"]
mod test_std_log;
#[path = "test_std_markdown.rs"]
mod test_std_markdown;
#[path = "test_std_math.rs"]
mod test_std_math;
#[path = "test_std_net.rs"]
mod test_std_net;
#[path = "test_std_plot.rs"]
mod test_std_plot;
#[path = "test_std_random.rs"]
mod test_std_random;
#[path = "test_std_ready.rs"]
mod test_std_ready;
#[path = "test_std_regex.rs"]
mod test_std_regex;
#[path = "test_std_report.rs"]
mod test_std_report;
mod test_std_report_sums;
#[path = "test_std_secrets.rs"]
mod test_std_secrets;
#[path = "test_std_shlex.rs"]
mod test_std_shlex;
#[path = "test_std_signal.rs"]
mod test_std_signal;
#[path = "test_std_simd.rs"]
mod test_std_simd;
#[path = "test_std_sketch.rs"]
mod test_std_sketch;
#[path = "test_std_sqlite.rs"]
mod test_std_sqlite;
#[path = "test_std_stat.rs"]
mod test_std_stat;
#[path = "test_std_sync_mutex.rs"]
mod test_std_sync_mutex;
#[path = "test_std_system.rs"]
mod test_std_system;
#[path = "test_std_tempfile.rs"]
mod test_std_tempfile;
#[path = "test_std_template.rs"]
mod test_std_template;
#[path = "test_std_test.rs"]
mod test_std_test;
#[path = "test_std_text_set_scans.rs"]
mod test_std_text_set_scans;
#[path = "test_std_text_unicode.rs"]
mod test_std_text_unicode;
#[path = "test_std_tls.rs"]
mod test_std_tls;
#[path = "test_std_toml.rs"]
mod test_std_toml;
#[path = "test_std_url.rs"]
mod test_std_url;
#[path = "test_std_uuid.rs"]
mod test_std_uuid;
#[path = "test_stdlib_index.rs"]
mod test_stdlib_index;
#[path = "test_storage_diagnostics.rs"]
mod test_storage_diagnostics;
#[path = "test_stored_closures.rs"]
mod test_stored_closures;
#[path = "test_symbol_references.rs"]
mod test_symbol_references;
#[path = "test_tail_abi.rs"]
mod test_tail_abi;
#[path = "test_tail_call_cleanup.rs"]
mod test_tail_call_cleanup;
#[path = "test_target_conditionals.rs"]
mod test_target_conditionals;
#[path = "test_text_bytes_fast_paths.rs"]
mod test_text_bytes_fast_paths;
#[path = "test_text_characters.rs"]
mod test_text_characters;
#[path = "test_text_queries.rs"]
mod test_text_queries;
#[path = "test_text_replacement.rs"]
mod test_text_replacement;
#[path = "test_text_transformations.rs"]
mod test_text_transformations;
#[path = "test_thread_callbacks.rs"]
mod test_thread_callbacks;
#[path = "test_threads.rs"]
mod test_threads;
#[path = "test_time.rs"]
mod test_time;
#[path = "test_toolchain.rs"]
mod test_toolchain;
#[path = "test_trap_locations.rs"]
mod test_trap_locations;
#[path = "test_tuple_borrowing.rs"]
mod test_tuple_borrowing;
#[path = "test_tuple_ordering.rs"]
mod test_tuple_ordering;
#[path = "test_tuples.rs"]
mod test_tuples;
#[path = "test_tutorial.rs"]
mod test_tutorial;
#[path = "test_type_cycles.rs"]
mod test_type_cycles;
#[path = "test_typed_ranges.rs"]
mod test_typed_ranges;
#[path = "test_typed_stream_prototype.rs"]
mod test_typed_stream_prototype;
#[path = "test_unsigned_literals.rs"]
mod test_unsigned_literals;
#[path = "test_values.rs"]
mod test_values;

/// Fails when a `tests/*.rs` file is not compiled into this binary.
#[test]
fn every_test_file_is_listed() {
    let listed: Vec<&str> = include_str!("integration.rs")
        .lines()
        .filter_map(|line| line.strip_prefix("mod ")?.strip_suffix(';'))
        .collect();
    let mut missing: Vec<String> = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/tests"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter_map(|name| Some(name.strip_suffix(".rs")?.to_owned()))
        .filter(|module| module != "integration" && !listed.contains(&module.as_str()))
        .collect();
    missing.sort();
    let entries: Vec<String> = missing
        .iter()
        .map(|module| format!("#[path = \"{module}.rs\"]\nmod {module};"))
        .collect();
    assert!(
        missing.is_empty(),
        "add to tests/integration.rs, in name order:\n{}",
        entries.join("\n")
    );
}

#[path = "test_std_xml.rs"]
mod test_std_xml;
