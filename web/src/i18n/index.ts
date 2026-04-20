export type Locale = 'en-US' | 'zh-CN';

const messages = {
  'en-US': {
    common: {
      login: 'Log in',
      logout: 'Log out',
      save: 'Save changes',
      saving: 'Saving...',
      cancel: 'Cancel',
      loading: 'Loading...',
    },
    settings: {
      title: 'User Settings',
      subtitle: 'Update your display name and application language.',
      languageLabel: 'Interface language',
      nicknameLabel: 'Display name',
      nicknameHint: 'This name is shown in the header and collaborative views.',
      languageHint: 'Changing the interface language updates the app immediately.',
      success: 'Settings saved successfully.',
      failed: 'Failed to save settings.',
      zhCN: '简体中文',
      enUS: 'English (US)',
    },
    organizations: {
      title: 'Select Organization',
      loading: 'Loading organizations...',
      failed: 'Failed to load organizations.',
      empty: 'No organizations found.',
      createTitle: 'Create organization',
      createSubtitle: 'Create a new workspace before adding projects.',
      name: 'Organization name',
      slug: 'Organization slug',
      description: 'Description',
      visibility: 'Visibility',
      createAction: 'Create organization',
      createSuccess: 'Organization created successfully.',
    },
    projects: {
      title: 'Select Project',
      loading: 'Loading projects...',
      failed: 'Failed to load projects.',
      empty: 'No projects found.',
      back: '← Back to Organizations',
      createTitle: 'Create project',
      createSubtitle: 'Only the organization creator can create projects in the current model.',
      name: 'Project name',
      slug: 'Project slug',
      description: 'Description',
      targetLanguage: 'Target language',
      sourceLanguages: 'Source languages',
      visibility: 'Visibility',
      guestPolicy: 'Guest policy',
      createAction: 'Create project',
      createSuccess: 'Project created successfully.',
    },
    jobs: {
      title: 'Import / Export Jobs',
      exportTitle: 'Export ZIP',
      exportAction: 'Request new export',
      exportLoading: 'Requesting export...',
      exportDownload: 'Download ZIP',
      exportEmpty: 'No export jobs found.',
      importTitle: 'Import JSON',
      importAction: 'Start Import',
      importLoading: 'Importing...',
      importEmpty: 'No import jobs found.',
      loginHint: 'You must be logged in to create new import/export jobs. You can still view the job history.',
      recentImports: 'Recent Imports',
      recentExports: 'Recent Exports',
      invalidJson: 'Invalid JSON file.',
    },
    loginPage: {
      title: 'PRTS Translation',
      email: 'Email',
      password: 'Password',
      loading: 'Logging in...',
      submit: 'Log in',
    },
  },
  'zh-CN': {
    common: {
      login: '登录',
      logout: '退出登录',
      save: '保存设置',
      saving: '保存中...',
      cancel: '取消',
      loading: '加载中...',
    },
    settings: {
      title: '个人设置',
      subtitle: '更新你的昵称和界面语言。',
      languageLabel: '界面语言',
      nicknameLabel: '昵称',
      nicknameHint: '该名称会展示在顶部导航和协作界面中。',
      languageHint: '切换后会立即更新当前界面文案。',
      success: '设置已保存。',
      failed: '保存设置失败。',
      zhCN: '简体中文',
      enUS: '英语（美国）',
    },
    organizations: {
      title: '选择组织',
      loading: '正在加载组织列表...',
      failed: '加载组织列表失败。',
      empty: '暂无组织。',
      createTitle: '创建组织',
      createSubtitle: '先创建组织，再在组织下创建项目。',
      name: '组织名称',
      slug: '组织标识',
      description: '描述',
      visibility: '可见性',
      createAction: '创建组织',
      createSuccess: '组织创建成功。',
    },
    projects: {
      title: '选择项目',
      loading: '正在加载项目列表...',
      failed: '加载项目列表失败。',
      empty: '暂无项目。',
      back: '← 返回组织列表',
      createTitle: '创建项目',
      createSubtitle: '在当前模型下，只有组织创建者可以创建项目。',
      name: '项目名称',
      slug: '项目标识',
      description: '描述',
      targetLanguage: '目标语言',
      sourceLanguages: '源语言',
      visibility: '可见性',
      guestPolicy: '访客策略',
      createAction: '创建项目',
      createSuccess: '项目创建成功。',
    },
    jobs: {
      title: '导入 / 导出任务',
      exportTitle: '导出 ZIP',
      exportAction: '申请新的导出',
      exportLoading: '正在申请导出...',
      exportDownload: '下载 ZIP',
      exportEmpty: '暂无导出任务。',
      importTitle: '导入 JSON',
      importAction: '开始导入',
      importLoading: '导入中...',
      importEmpty: '暂无导入任务。',
      loginHint: '只有登录后才能创建新的导入/导出任务，但仍可以查看历史记录。',
      recentImports: '最近导入',
      recentExports: '最近导出',
      invalidJson: 'JSON 文件格式无效。',
    },
    loginPage: {
      title: 'PRTS 翻译平台',
      email: '邮箱',
      password: '密码',
      loading: '正在登录...',
      submit: '登录',
    },
  },
} as const;

type MessageTree = typeof messages['en-US'];

export function normalizeLocale(value?: string | null): Locale {
  return value === 'zh-CN' ? 'zh-CN' : 'en-US';
}

export function translate(locale: string | null | undefined, key: string): string {
  const resolvedLocale = normalizeLocale(locale);
  const parts = key.split('.');

  const resolveFrom = (tree: MessageTree | Record<string, unknown>) => {
    let current: unknown = tree;
    for (const part of parts) {
      if (!current || typeof current !== 'object' || !(part in current)) {
        return undefined;
      }
      current = (current as Record<string, unknown>)[part];
    }
    return typeof current === 'string' ? current : undefined;
  };

  return resolveFrom(messages[resolvedLocale]) ?? resolveFrom(messages['en-US']) ?? key;
}
