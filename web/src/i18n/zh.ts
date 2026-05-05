const common = {
    loading: "加载中...",
};
const sidebar = {
    home: "首页",
    userFeature: "用户",
    user: "用户",
    loginHistory: "登录历史",
    modelFeature: "模型",
    file: "文件",
    config: "配置",
    httpStat: "Http统计",
    httpDetector: "Http检测器",
    webPageDetector: "网页检测器",
    detectorGroup: "用户分组",
    detectorGroupUser: "用户分组用户",
    tokenFeature: "Token 计费",
    tokenAccount: "账户余额",
    tokenRecharge: "充值记录",
    tokenUsage: "消耗记录",
    tokenKey: "API 密钥",
    tokenPrice: "定价配置",
};
const login = {
    title: "登录",
    description: "请在下方输入您的账户信息进行登录",
    account: "账户",
    accountPlaceholder: "请输入您的账户",
    password: "密码",
    passwordPlaceholder: "请输入您的密码",
    forgotPassword: "忘记密码？",
    submit: "登录",
    dontHaveAccount: "还没有账户？",
    captcha: "验证码",
    signUp: "注册",
};
const signUp = {
    title: "注册",
    description: "请在下方输入您的账户信息进行注册",
    account: "账户",
    accountPlaceholder: "请输入您的账户",
    password: "密码",
    passwordPlaceholder: "请输入您的密码",
    confirmPassword: "确认密码",
    confirmPasswordPlaceholder: "请确认您的密码",
    submit: "注册",
    haveAccount: "已有账户？",
    login: "登录",
    success: "注册成功，请登录",
};
const appUser = {
    themeSystem: "系统主题",
    themeDark: "深色主题",
    themeLight: "浅色主题",
    login: "登录",
    logout: "退出登录",
    profile: "个人资料",
};

const profile = {
    title: "个人资料",
    email: "邮箱",
    emailPlaceholder: "请输入您的邮箱",
    avatar: "头像",
    avatarPlaceholder: "请输入您的头像",
    roles: "角色",
    rolesPlaceholder: "请选择您的角色",
    groups: "群组",
    groupsPlaceholder: "请选择您的群组",
    submit: "更新资料",
    updateSuccess: "资料更新成功",
};

const model = {
    noRecords: "未找到记录",
    rowsPerPage: "每页行数",
    selectRowsPerPage: "选择行数",
    pageContent: "第 {page} 页，共 {total} 页",
    keywordPlaceholder: "请输入关键词",
    filter: "筛选",
    active: "激活",
    inactive: "未激活",
    success: "成功",
    failed: "失败",
    select: "选择",
    input: "输入",
    nextPage: "下一页",
    previousPage: "上一页",
    columns: "列",
    view: "查看",
    edit: "编辑",
    delete: "删除",
    deleteTitle: "您确定要删除吗？",
    deleteDescription:
        "此操作无法撤销。这将永久删除您的账户并从我们的服务器中移除您的数据。",
    cancel: "取消",
    continue: "继续",
    create: "创建",
};

const modelEditor = {
    update: "更新",
    create: "创建",
    back: "返回",
    noChange: "没有更改需要更新",
    updateSuccess: "更新成功",
    createSuccess: "创建成功",
};

const component = {
    now: "现在",
    pickDate: "选择日期",
    dragUpload: "拖拽文件到此处",
    clickUpload: "或点击浏览（最多2个文件，每个文件最大5MB）",
    browseFiles: "浏览文件",
    upload: "上传",
    uploadedSuccessfully: "上传成功",
    group: "群组",
    selectGroup: "选择一个群组",
    files: "文件",
};

export default {
    common,
    sidebar,
    login,
    signUp,
    appUser,
    profile,
    model,
    modelEditor,
    component,
};
