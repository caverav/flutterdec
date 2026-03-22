Widget _buildPingBadge(int? ping) {
  if (ping == null) return const SizedBox(width: 50);

  return Container(
    constraints: const BoxConstraints(minWidth: 50),
    padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
    decoration: BoxDecoration(
      color: AppTheme.getPingColor(ping).withOpacity(0.15),
      borderRadius: BorderRadius.circular(8),
    ),
    child: Text(
      ping >= 0 ? '${ping}ms' : 'Fail',
      textAlign: TextAlign.center,
    ),
  );
}
