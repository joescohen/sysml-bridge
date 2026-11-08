//! Round 36, security item 0: limits come before the work they bound. Two of these fail with the fix off: the request-size test
//! (the old code read the scope and the rows first, so it answered with a different message) and the page test (the old
//! reply has no `steps`, and it computed every cell of every row).


