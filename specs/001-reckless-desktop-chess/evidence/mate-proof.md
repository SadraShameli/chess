# Bounded mate-proof implementation and limits

T052 now includes 21 start/adjudication cases. Its typed fixtures cover legal and
illegal en-passant, four promotions, Standard castling, stationary/swap Chess960
castling, independent DFRC rights, intended fifty-move claims and reset behavior,
and side-specific flagfall. Existing terminal results take precedence over a
subsequent flagfall request. Invalid fixture positions were corrected after
shakmaty rejected opposite-check and a terminal stalemate expectation.

T062 has a bounded implementation and ten passing focused cases, but remains
unchecked because practical general-position coverage is unresolved. Conservative
library material certificates, verified legal mate witnesses and finite complete
reachability can establish Impossible or Possible. Exhausted time, state or
allocation limits and cancellation always return Unknown. A common middlegame
may therefore require a preserved interruption rather than receive a false draw
or an unsupported possible-mate certificate. This is a release blocker.

The selected maximums are two seconds, one million states and 128 MiB. Tests
inject clocks/cancellation and exercise each limit. Memoization retains complete
position identities, including rights and legal en-passant, rather than trusting
hash equality. Proof transitions normalize counters without formatting FEN in
the search loop. Witness suffix reuse requires exact next-move position identity.

The pinned hashbrown 0.16.1 dependency already existed transitively. Its verified
allocation_size API supports conservative accounting of owned hash storage;
arena, scratch and witness capacities are included. This is not a process-RSS
measurement, performance benchmark or proof of practical arbitrary-position
coverage. Application integration must execute the synchronous proof on the
shared bounded CPU executor and account for retained witness caches separately.

The combined debug domain suite passed 129 tests. Targeted Clippy passed. The
existing macOS linker warning remains recorded in the engine evidence. No
optimized or cross-platform performance acceptance was run.
