import js from '@eslint/js'
import tseslint from 'typescript-eslint'
import pluginVue from 'eslint-plugin-vue'
import configPrettier from 'eslint-config-prettier'

// ESLint 9 扁平配置（flat config）。Prettier 负责排版，eslint-config-prettier 关闭与之冲突的格式规则。
export default tseslint.config(
  { ignores: ['dist/**', 'node_modules/**'] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  ...pluginVue.configs['flat/recommended'],
  {
    // .vue 文件的 <script> 用 TS 解析器解析。
    files: ['**/*.vue'],
    languageOptions: {
      parserOptions: { parser: tseslint.parser },
    },
  },
  {
    rules: {
      'vue/multi-word-component-names': 'off',
    },
  },
  // 必须放最后：关闭所有与 Prettier 冲突的格式化规则。
  configPrettier,
)
