#!/bin/bash
#
# Runs all fuzz targets for extended periods (1hr each by default)
# Usage:
#   ./scripts/fuzz-local.sh           # Run all targets for 1hr each
#   ./scripts/fuzz-local.sh 300       # Run all targets for 5min each
#   ./scripts/fuzz-local.sh quick     # Run all targets for 60s each (quick check)

set -e
set -o pipefail

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Default: 1 hour per target
FUZZ_TIME=${1:-3600}

# Handle "quick" mode
if [ "$1" == "quick" ]; then
    FUZZ_TIME=60
    echo -e "${YELLOW}Quick mode: Running each fuzzer for 60 seconds${NC}"
elif [ -n "$1" ]; then
    if ! [[ "$1" =~ ^[0-9]+$ ]]; then
        echo -e "${RED}Error: Duration must be a positive integer or 'quick'${NC}"
        echo -e "Usage: $0 [seconds|quick]"
        exit 1
    elif [ "$1" -eq 0 ]; then
        echo -e "${RED}Error: Duration must be greater than 0${NC}"
        exit 1
    fi
    echo -e "${YELLOW}Running each fuzzer for ${FUZZ_TIME} seconds${NC}"
else
    echo -e "${YELLOW}Running each fuzzer for 1 hour (3600s)${NC}"
fi

# Fuzzer targets in priority order
TARGETS=(
    "mls_signature_fuzzer"
    "connection_state_fuzzer"
    "frame_boundary_fuzzer"
    "room_manager_fuzzer"
    "sequencer_state_fuzzer"
    "e2e_pipeline_fuzzer"
    "cbor_attack_fuzzer"
)

# Track results
PASSED=0
FAILED=0
FAILED_TARGETS=()

echo -e "${BLUE}=== Lockframe Fuzzing Suite ===${NC}"
echo -e "Targets: ${#TARGETS[@]}"
echo -e "Time per target: ${FUZZ_TIME}s"
echo ""

# Run each fuzzer
for target in "${TARGETS[@]}"; do
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${BLUE}Fuzzing: $target${NC}"
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

    if cargo fuzz run "$target" -- -max_total_time="$FUZZ_TIME" 2>&1 | tee "fuzz-$target.log"; then
        echo -e "${GREEN}✓ $target: PASSED${NC}"
        PASSED=$((PASSED + 1))
    else
        echo -e "${RED}✗ $target: FAILED${NC}"
        FAILED=$((FAILED + 1))
        FAILED_TARGETS+=("$target")
    fi

    echo ""
done

# Summary
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}Fuzzing Complete${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "Passed: ${GREEN}$PASSED${NC}"
echo -e "Failed: ${RED}$FAILED${NC}"

if [ $FAILED -gt 0 ]; then
    echo ""
    echo -e "${RED}Failed targets:${NC}"
    for failed_target in "${FAILED_TARGETS[@]}"; do
        echo -e "  - $failed_target (see fuzz-$failed_target.log)"
    done
    echo ""
    echo -e "${YELLOW}To investigate:${NC}"
    echo -e "  1. Check artifacts: ls -la fuzz/artifacts/<target>/"
    echo -e "  2. Replay crash: cargo fuzz run <target> fuzz/artifacts/<target>/crash-*"
    echo -e "  3. Review log: cat fuzz-<target>.log"
    exit 1
else
    echo -e "${GREEN}All fuzzers passed! ✓${NC}"
    echo ""
    echo -e "${YELLOW}Next steps:${NC}"
    echo -e "  - Minimize corpus: cargo fuzz cmin <target>"
    echo -e "  - Check coverage: cargo fuzz coverage <target>"
    echo -e "  - Run for longer: $0 7200  # 2 hours per target"
    exit 0
fi
