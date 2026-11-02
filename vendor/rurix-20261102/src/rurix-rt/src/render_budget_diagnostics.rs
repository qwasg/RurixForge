// Diagnostic-only addition for the existing mandatory post-frame budget gate.
// No query, predicate, budget, allocation, retry or failure disposition changes.
// RFC-0052; Assisted-by: Codex:GPT-6.
fn budget_failure_diagnostic(
    reason:&str,heap_sizes:&[u64],raw_budgets:&[u64],raw_usages:&[u64],heaps:&[HeapBudgetTelemetry],
) -> String {
    let rows=heaps.iter().map(|h| {
        let index=h.heap_index as usize;
        let optional=|values:&[u64]|values.get(index).map_or("null".into(),u64::to_string);
        let raw_budget=optional(raw_budgets);let raw_usage=optional(raw_usages);
        let usage_minus_ledger=i128::from(h.driver_usage_bytes)-i128::from(h.ledger_bytes);
        let budget_minus_usage=i128::from(h.budget_bytes)-i128::from(h.driver_usage_bytes);
        format!("{{\"heap_index\":{},\"heap_size_bytes\":{},\"raw_budget_bytes\":{raw_budget},\"raw_usage_bytes\":{raw_usage},\"effective_budget_bytes\":{},\"ledger_bytes\":{},\"usage_minus_ledger_bytes\":{usage_minus_ledger},\"budget_minus_usage_bytes\":{budget_minus_usage},\"zero_budget\":{},\"usage_exceeds_budget\":{}}}",
            h.heap_index,optional(heap_sizes),h.budget_bytes,h.ledger_bytes,h.budget_bytes==0,h.driver_usage_bytes>h.budget_bytes)
    }).collect::<Vec<_>>().join(",");
    format!("VK_EXT_memory_budget 返回空/非法 heap 实值(fail-closed); memory_budget_diagnostic={{\"schema\":\"rurix.render.memory_budget_failure.v1\",\"query_location\":\"collect_persistent_frame_after_fence_timestamps_and_host_readback\",\"reason\":\"{reason}\",\"memory_heap_count\":{},\"telemetry_heap_count\":{},\"heaps\":[{rows}],\"interpretation\":\"driver_usage_is_an_estimate;usage_minus_ledger_is_not_proof_of_driver_private_allocation;budget_failure_still_rejects_frame\"}}",
        heap_sizes.len(),heaps.len())
}

#[cfg(test)]
mod memory_budget_failure_diagnostic_tests {
    use super::*;
    #[test]
    fn distinguishes_empty_zero_and_over_budget_without_altering_values() {
        let empty=budget_failure_diagnostic("empty_or_zero_budget",&[],&[],&[],&[]);
        assert!(empty.contains("\"telemetry_heap_count\":0"));
        let heaps=[HeapBudgetTelemetry{heap_index:0,budget_bytes:100,driver_usage_bytes:130,ledger_bytes:120},
            HeapBudgetTelemetry{heap_index:1,budget_bytes:0,driver_usage_bytes:4,ledger_bytes:2}];
        let message=budget_failure_diagnostic("driver_usage_exceeds_budget",&[150,250],&[110,0],&[130,4],&heaps);
        assert!(message.starts_with("VK_EXT_memory_budget 返回空/非法 heap 实值(fail-closed)"));
        assert!(message.contains("\"heap_size_bytes\":150,\"raw_budget_bytes\":110,\"raw_usage_bytes\":130,\"effective_budget_bytes\":100,\"ledger_bytes\":120"));
        assert!(message.contains("\"usage_minus_ledger_bytes\":10,\"budget_minus_usage_bytes\":-30,\"zero_budget\":false,\"usage_exceeds_budget\":true"));
        assert!(message.contains("\"zero_budget\":true,\"usage_exceeds_budget\":true"));
        assert_eq!(heaps[0].budget_bytes,100);assert_eq!(heaps[0].driver_usage_bytes,130);
    }
    #[test]
    fn signed_difference_does_not_claim_private_memory_or_underflow() {
        let heaps=[HeapBudgetTelemetry{heap_index:0,budget_bytes:400,driver_usage_bytes:50,ledger_bytes:80}];
        let message=budget_failure_diagnostic("empty_or_zero_budget",&[500],&[400],&[50],&heaps);
        assert!(message.contains("\"usage_minus_ledger_bytes\":-30,\"budget_minus_usage_bytes\":350"));
        assert!(message.contains("usage_minus_ledger_is_not_proof_of_driver_private_allocation"));
    }
    #[test]
    fn malformed_diagnostic_table_never_fabricates_missing_values() {
        let heaps=[HeapBudgetTelemetry{heap_index:3,budget_bytes:0,driver_usage_bytes:1,ledger_bytes:0}];
        let message=budget_failure_diagnostic("empty_or_zero_budget",&[],&[],&[],&heaps);
        assert!(message.contains("\"heap_size_bytes\":null,\"raw_budget_bytes\":null,\"raw_usage_bytes\":null"));
    }
}
