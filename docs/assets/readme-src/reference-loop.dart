int? incrementUntilNonZero(int receiver) {
  while (true) {
    final next = receiver + 1;
    if (next != 0) {
      return next;
    }
  }
}
