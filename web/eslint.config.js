import js from '@eslint/js'
import { defineConfig, globalIgnores } from 'eslint/config'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import globals from 'globals'
import tseslint from 'typescript-eslint'

export default defineConfig([
  globalIgnores(['dist', 'coverage', 'src/generated']),
  {
    files: ['**/*.{ts,tsx}'],
    extends: [
      js.configs.recommended,
      ...tseslint.configs.recommendedTypeChecked,
      reactHooks.configs.flat['recommended-latest'],
    ],
    plugins: {
      'react-refresh': reactRefresh,
    },
    languageOptions: {
      ecmaVersion: 'latest',
      globals: globals.browser,
      parserOptions: {
        // 类型感知 lint：用 projectService 自动为每个文件找到所属 tsconfig。
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    rules: {
      'react-refresh/only-export-components': ['error', { allowConstantExport: true }],
      // 未使用变量以下划线前缀显式标注“故意忽略”。
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_', caughtErrorsIgnorePattern: '^_' },
      ],
    },
  },
  {
    // 密钥与 WASM 只能存在于 Web Worker：主线程代码不得直接 import。
    files: ['src/**/*.{ts,tsx}'],
    ignores: ['src/worker/**'],
    rules: {
      'no-restricted-imports': [
        'error',
        {
          patterns: [
            {
              group: ['**/generated/crypto-wasm/**', '**/crypto-backend'],
              message:
                'WASM 与密钥只能存在于 Web Worker；主线程请使用 src/worker/client.ts',
            },
          ],
        },
      ],
    },
  },
])
