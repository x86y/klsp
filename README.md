# A language server for the K programming language.

Klsp can be used for any editor that supports LSP. To use it in neovim, add the following code to your config:

```lua
local configs = require('lspconfig.configs')
local util = require('lspconfig/util')
configs.k = {
    default_config = {
        cmd = { 'PATH_TO_KLSP' },
        cmd_env = {},
        filetypes = { 'k' },
        root_dir = require('lspconfig.configs').find_git_ancestor,
        single_file_support = true,
    },
}
require("lspconfig")["k"].setup { }
```
