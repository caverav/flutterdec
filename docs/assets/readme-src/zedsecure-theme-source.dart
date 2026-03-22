static ThemeData lightTheme() {
  return ThemeData(
    brightness: Brightness.light,
    primaryColor: primaryBlue,
    scaffoldBackgroundColor: systemGray6,
    appBarTheme: const AppBarTheme(
      backgroundColor: Colors.transparent,
      elevation: 0,
      iconTheme: IconThemeData(color: primaryBlue),
    ),
    colorScheme: const ColorScheme.light(
      primary: primaryBlue,
      secondary: connectedGreen,
    ),
  );
}
