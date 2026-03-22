static bool _isTwoByteWhitespace(int codeUnit) {
  if (codeUnit <= 32) {
    return (codeUnit == 32) || ((codeUnit <= 13) && (codeUnit >= 9));
  }
  if (codeUnit < 0x85) return false;
  if ((codeUnit == 0x85) || (codeUnit == 0xA0)) return true;
  return (codeUnit <= 0x200A)
      ? ((codeUnit == 0x1680) || (0x2000 <= codeUnit))
      : ((codeUnit == 0x2028) ||
            (codeUnit == 0x2029) ||
            (codeUnit == 0x202F) ||
            (codeUnit == 0x205F) ||
            (codeUnit == 0x3000) ||
            (codeUnit == 0xFEFF));
}
