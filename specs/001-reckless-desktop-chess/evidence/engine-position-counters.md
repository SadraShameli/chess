# Engine position counter preservation

Report integration found that the analysis position serializer selected
`position startpos` through BoardPosition equality. Its dependency equality
excludes move counters, so a Standard arrangement with halfmove 7 and fullmove
1 lost those fields at the engine boundary.

The independently registered regression failed against the unchanged
serializer: zero passed, one failed, 235 filtered. Exact source/test blobs and
the failure log are preserved in
`.git/parallel-patches/engine-position-counter-baseline-20261017/`.
The log SHA-256 is
`e6ff470e307d424a456eb0f60e7a8a8ba6a17f88c60c126f661b2cc269edcc8a`.

The serializer now permits startpos only when the declared variant is Standard
and the entire canonical FEN equals Standard setup. Other starts use that
actual canonical FEN, including counters, followed by the validated complete
move history. BoardPosition equality and repetition identity are unchanged.

The new regression checks custom halfmove/fullmove combinations both at the
root and with a continuation. Its default Standard control still uses
startpos. All 14 surrounding engine lifecycle checks passed, with zero failed
or ignored and 222 filtered. The log SHA-256 is
`612632d46662af641014aad1e445ed5033e365771eea2d70beaa5db33ffd1347`.

The report foundation consequently supports custom counters and retains its
full canonical FEN and variant checks. This evidence does not accept report
persistence/scheduling, native controls or complete engine/platform coverage.
