import 'package:encrypted_backup_client/app.dart';
import 'package:encrypted_backup_client/core/router/app_router.dart';
import 'package:encrypted_backup_client/core/router/destinations.dart';
import 'package:encrypted_backup_client/desktop/router/desktop_routes.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  testWidgets('主窗口骨架显示全部侧栏目的地并可切换路由', (tester) async {
    // 测试运行在 Linux/Windows/macOS 主机上：显式注入桌面路由，保证组件
    // 测试始终覆盖完整侧栏（生产入口按平台过滤）。
    final router = buildAppRouter(
      destinations: appDestinations,
      extraShellRoutes: [backupFoldersRoute],
    );
    await tester.pumpWidget(
      ProviderScope(child: EncryptedBackupApp(router: router)),
    );
    await tester.pumpAndSettle();

    for (final label in const ['全部文件', '最近', '回收站', '备份文件夹', '安全与隐私', '设置']) {
      expect(find.text(label), findsWidgets, reason: '侧栏缺少 $label');
    }

    await tester.tap(find.text('备份文件夹').first);
    await tester.pumpAndSettle();
    expect(find.textContaining('添加本地文件夹'), findsOneWidget);

    await tester.tap(find.text('设置').first);
    await tester.pumpAndSettle();
    expect(find.textContaining('保持解锁'), findsWidgets);
  });
}
