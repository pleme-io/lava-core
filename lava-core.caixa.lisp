(defcaixa
  :name
  "lava-core"
  :kind
  :Biblioteca
  :ecosystem
  :rust-single-crate
  :package
  {:name "lava-core"
   :version "0.1.0"
   :description "Typed primitive layer for the lava suite. Tatara-lisp + Rust DSL frontend for magma. Brazilian-Portuguese for the substance magma flows as. Sits on pleme-io/magma as the tatara equivalent of pangea-core."
   :license "MIT"
   :repository "https://github.com/pleme-io/lava-core"}
  :ci-config
  {:bump {:default-type "patch"}
   :publish {:no-verify true}}
  :workflows
  [:auto-release :pre-merge-gate :security-gate])
