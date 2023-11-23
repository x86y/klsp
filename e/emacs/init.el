; UNTESTED

(require 'package)
(add-to-list 'package-archives '("melpa" . "https://melpa.org/packages/") t)
(package-initialize)
(unless (package-installed-p 'lsp-mode)
  (package-refresh-contents)
  (package-install 'lsp-mode))

(require 'lsp-mode)
(add-to-list 'lsp-language-id-configuration '(k-language . "k"))
(lsp-register-client
 (make-lsp-client :new-connection (lsp-stdio-connection "path-to-klsp-binary") ; EDIT THE PATH
                  :major-modes '(k-mode)
                  :server-id 'klsp))
(add-hook 'k-mode-hook #'lsp)

