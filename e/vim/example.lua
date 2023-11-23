local configs = require('lspconfig.configs')
local util = require('lspconfig/util')
configs.k = {
    default_config = {
        cmd = { 'PATH_TO_KLSP' },
        cmd_env = {},
        filetypes = { 'k' },
        root_dir = util.find_git_ancestor,
        single_file_support = true,
    },
}
require("lspconfig")["k"].setup { }

