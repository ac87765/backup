import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/rust/models.dart';
import '../../core/theme/design_tokens.dart';

/// 文件类型视觉分类（对应 tokens.css `.ficon.*`）。
enum FileVisualKind {
  folder,
  pdf,
  doc,
  sheet,
  slides,
  image,
  video,
  archive,
  other,
}

FileVisualKind fileVisualKind({required String name, required bool folder}) {
  if (folder) {
    return FileVisualKind.folder;
  }
  final dot = name.lastIndexOf('.');
  final extension = dot > 0 ? name.substring(dot + 1).toLowerCase() : '';
  return switch (extension) {
    'pdf' => FileVisualKind.pdf,
    'doc' || 'docx' || 'rtf' || 'odt' || 'txt' || 'md' => FileVisualKind.doc,
    'xls' || 'xlsx' || 'csv' || 'ods' => FileVisualKind.sheet,
    'ppt' || 'pptx' || 'odp' || 'key' => FileVisualKind.slides,
    'jpg' ||
    'jpeg' ||
    'png' ||
    'gif' ||
    'webp' ||
    'heic' ||
    'bmp' ||
    'svg' => FileVisualKind.image,
    'mp4' || 'mov' || 'avi' || 'mkv' || 'webm' || 'm4v' => FileVisualKind.video,
    'zip' ||
    'rar' ||
    '7z' ||
    'tar' ||
    'gz' ||
    'bz2' ||
    'xz' => FileVisualKind.archive,
    _ => FileVisualKind.other,
  };
}

({IconData icon, Color foreground, Color background}) fileVisualStyle(
  FileVisualKind kind,
  DesktopTokens tokens,
) {
  return switch (kind) {
    FileVisualKind.folder => (
      icon: LucideIcons.folder,
      foreground: tokens.fileFolder,
      background: tokens.fileFolderBg,
    ),
    FileVisualKind.pdf => (
      icon: LucideIcons.fileText,
      foreground: tokens.filePdf,
      background: tokens.danger50,
    ),
    FileVisualKind.doc => (
      icon: LucideIcons.fileText,
      foreground: tokens.fileDoc,
      background: tokens.fileDocBg,
    ),
    FileVisualKind.sheet => (
      icon: LucideIcons.sheet,
      foreground: tokens.fileSheet,
      background: tokens.fileSheetBg,
    ),
    FileVisualKind.slides => (
      icon: LucideIcons.presentation,
      foreground: tokens.fileSlides,
      background: tokens.fileArchiveBg,
    ),
    FileVisualKind.image => (
      icon: LucideIcons.image,
      foreground: tokens.fileImage,
      background: tokens.success50,
    ),
    FileVisualKind.video => (
      icon: LucideIcons.video,
      foreground: tokens.fileVideo,
      background: tokens.fileVideoBg,
    ),
    FileVisualKind.archive => (
      icon: LucideIcons.archive,
      foreground: tokens.fileArchive,
      background: tokens.fileArchiveBg,
    ),
    FileVisualKind.other => (
      icon: LucideIcons.file,
      foreground: tokens.fileOther,
      background: tokens.canvas,
    ),
  };
}

/// 28×28 文件类型图标块。
class FileIconTile extends StatelessWidget {
  const FileIconTile({
    required this.name,
    this.folder = false,
    this.size = DesktopMetrics.iconTileSize,
    this.iconSize = 16,
    super.key,
  });

  factory FileIconTile.node(
    NodeDto node, {
    double size = DesktopMetrics.iconTileSize,
    double iconSize = 16,
    Key? key,
  }) {
    return FileIconTile(
      name: node.name,
      folder: node.kind == NodeKindDto.folder,
      size: size,
      iconSize: iconSize,
      key: key,
    );
  }

  final String name;
  final bool folder;
  final double size;
  final double iconSize;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final style = fileVisualStyle(
      fileVisualKind(name: name, folder: folder),
      tokens,
    );
    return Container(
      width: size,
      height: size,
      decoration: BoxDecoration(
        color: style.background,
        borderRadius: BorderRadius.circular(6),
      ),
      child: Icon(style.icon, size: iconSize, color: style.foreground),
    );
  }
}
