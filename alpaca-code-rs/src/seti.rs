//! seti.rs — file-type icons, from jesseweed/seti-ui (MIT). Assets are BAKED
//! one-shot (converter over seti-ui mapping.less + ui-variables + icons/*.svg):
//! each seti glyph is flattened to the single color its rule paints — seti-ui
//! renders these as font glyphs, one theme color per rule, so the svgs' own
//! fills/classes (sketch export) are overridden, `<style>` blocks are stripped
//! (CSS fill would beat the baked `fill` attr) and gradients flatten to the
//! rule color. Rasterized for pixbuf cells by badges.rs (librsvg); never
//! parsed by vector.rs's art-walker.// seti-default-white = #d4d7d6
// seti-bsl-red = #cc3e44
// seti-mdo-red = #cc3e44
// seti-salesforce-blue = #519aba
// seti-asm-red = #cc3e44
// seti-bicep-blue = #519aba
// seti-bazel-green = #8dc149
// seti-c-blue = #519aba
// seti-c-purple = #a074c4
// seti-c-yellow = #cbcb41
// seti-c-sharp-blue = #519aba
// seti-html-blue = #519aba
// seti-html-green = #8dc149
// seti-html-yellow = #cbcb41
// seti-cpp-blue = #519aba
// seti-cpp-purple = #a074c4
// seti-cpp-yellow = #cbcb41
// seti-clojure-green = #8dc149
// seti-clojure-blue = #519aba
// seti-coldfusion-blue = #519aba
// seti-coffee-yellow = #cbcb41
// seti-config-grey-light = #6d8086
// seti-crystal-white = #d4d7d6
// seti-crystal_embedded-white = #d4d7d6
// seti-json-yellow = #cbcb41
// seti-css-blue = #519aba
// seti-csv-green = #8dc149
// seti-xls-green = #8dc149
// seti-cu-green = #8dc149
// seti-cu-purple = #a074c4
// seti-cake-red = #cc3e44
// seti-cake_php-red = #cc3e44
// seti-d-red = #cc3e44
// seti-word-blue = #519aba
// seti-ejs-yellow = #cbcb41
// seti-elixir-purple = #a074c4
// seti-elixir_script-purple = #a074c4
// seti-hex-red = #cc3e44
// seti-elm-blue = #519aba
// seti-favicon-yellow = #cbcb41
// seti-f-sharp-blue = #519aba
// seti-git-ignore = #41535b
// seti-go2-blue = #519aba
// seti-go-blue = #519aba
// seti-godot-blue = #519aba
// seti-godot-red = #cc3e44
// seti-godot-yellow = #cbcb41
// seti-godot-purple = #a074c4
// seti-gradle-blue = #519aba
// seti-grails-green = #8dc149
// seti-graphql-pink = #f55385
// seti-hacklang-orange = #e37933
// seti-haml-red = #cc3e44
// seti-mustache-orange = #e37933
// seti-haskell-purple = #a074c4
// seti-haxe-orange = #e37933
// seti-haxe-yellow = #cbcb41
// seti-haxe-blue = #519aba
// seti-haxe-purple = #a074c4
// seti-html-orange = #e37933
// seti-jade-red = #cc3e44
// seti-java-red = #cc3e44
// seti-java-blue = #519aba
// seti-javascript-yellow = #cbcb41
// seti-javascript-orange = #e37933
// seti-jinja-red = #cc3e44
// seti-julia-purple = #a074c4
// seti-karma-green = #8dc149
// seti-kotlin-orange = #e37933
// seti-dart-blue = #519aba
// seti-less-blue = #519aba
// seti-liquid-green = #8dc149
// seti-livescript-blue = #519aba
// seti-lua-blue = #519aba
// seti-markdown-blue = #519aba
// seti-argdown-blue = #519aba
// seti-info-blue = #519aba
// seti-clock-blue = #519aba
// seti-maven-red = #cc3e44
// seti-nim-yellow = #cbcb41
// seti-github-white = #d4d7d6
// seti-notebook-blue = #519aba
// seti-nunjucks-green = #8dc149
// seti-npm-ignore = #41535b
// seti-npm-red = #cc3e44
// seti-ocaml-orange = #e37933
// seti-odata-orange = #e37933
// seti-perl-blue = #519aba
// seti-php-purple = #a074c4
// seti-pipeline-orange = #e37933
// seti-pddl-purple = #a074c4
// seti-plan-green = #8dc149
// seti-happenings-blue = #519aba
// seti-powershell-blue = #519aba
// seti-prisma-blue = #519aba
// seti-pug-red = #cc3e44
// seti-puppet-yellow = #cbcb41
// seti-purescript-white = #d4d7d6
// seti-python-blue = #519aba
// seti-react-blue = #519aba
// seti-react-orange = #e37933
// seti-reasonml-red = #cc3e44
// seti-rescript-red = #cc3e44
// seti-rescript-pink = #f55385
// seti-R-blue = #519aba
// seti-ruby-red = #cc3e44
// seti-html_erb-red = #cc3e44
// seti-rust-grey-light = #6d8086
// seti-sass-pink = #f55385
// seti-spring-green = #8dc149
// seti-slim-orange = #e37933
// seti-smarty-yellow = #cbcb41
// seti-sbt-blue = #519aba
// seti-scala-red = #cc3e44
// seti-ethereum-blue = #519aba
// seti-stylus-green = #8dc149
// seti-svelte-red = #cc3e44
// seti-swift-orange = #e37933
// seti-db-pink = #f55385
// seti-db-blue = #519aba
// seti-terraform-purple = #a074c4
// seti-tex-blue = #519aba
// seti-tex-yellow = #cbcb41
// seti-tex-orange = #e37933
// seti-tex-white = #d4d7d6
// seti-twig-green = #8dc149
// seti-typescript-blue = #519aba
// seti-typescript-orange = #e37933
// seti-tsconfig-blue = #519aba
// seti-vala-grey-light = #6d8086
// seti-vite-yellow = #cbcb41
// seti-vue-green = #8dc149
// seti-wasm-purple = #a074c4
// seti-wat-purple = #a074c4
// seti-xml-orange = #e37933
// seti-yml-purple = #a074c4
// seti-json-green = #8dc149
// seti-prolog-orange = #e37933
// seti-zig-orange = #e37933
// seti-zip-red = #cc3e44
// seti-zip-grey-light = #6d8086
// seti-wgt-blue = #519aba
// seti-illustrator-yellow = #cbcb41
// seti-photoshop-blue = #519aba
// seti-pdf-red = #cc3e44
// seti-font-red = #cc3e44
// seti-image-purple = #a074c4
// seti-svg-purple = #a074c4
// seti-sublime-orange = #e37933
// seti-code-search-purple = #a074c4
// seti-shell-green = #8dc149
// seti-video-pink = #f55385
// seti-audio-purple = #a074c4
// seti-svg-blue = #519aba
// seti-windows-blue = #519aba
// seti-jenkins-red = #cc3e44
// seti-babel-yellow = #cbcb41
// seti-bazel-grey = #4d5a5e
// seti-bower-orange = #e37933
// seti-docker-blue = #519aba
// seti-docker-grey = #4d5a5e
// seti-docker-green = #8dc149
// seti-docker-pink = #f55385
// seti-code-climate-green = #8dc149
// seti-eslint-purple = #a074c4
// seti-eslint-grey = #4d5a5e
// seti-firebase-orange = #e37933
// seti-firefox-orange = #e37933
// seti-gitlab-orange = #e37933
// seti-grunt-orange = #e37933
// seti-gulp-red = #cc3e44
// seti-ionic-blue = #519aba
// seti-javascript-blue = #519aba
// seti-platformio-orange = #e37933
// seti-rollup-red = #cc3e44
// seti-stylelint-white = #d4d7d6
// seti-stylelint-grey = #4d5a5e
// seti-yarn-blue = #519aba
// seti-webpack-blue = #519aba
// seti-clock-grey-light = #6d8086
// seti-lock-green = #8dc149
// seti-license-yellow = #cbcb41
// seti-license-orange = #e37933
// seti-license-red = #cc3e44
// seti-makefile-orange = #e37933
// seti-makefile-purple = #a074c4
// seti-makefile-grey-light = #6d8086
// seti-makefile-blue = #519aba
// seti-heroku-purple = #a074c4
// seti-todo-seti-primary = #519aba
// seti-npm_ignored-ignore = #41535b
// seti-ignored-ignore = #41535b

// ---- rust ----
#[rustfmt::skip]
pub(crate) const RULES: &[(&str, &str, bool)] = &[
    // (suffix-or-substring pattern, seti asset stem, is_partial) —
    // seti-ui styles/components/icons/mapping.less cascade order: LAST
    // match wins (defaults first, specifics later), partial = substring.
    ("", "seti-default-white.svg", false),
    (".bsl", "seti-bsl-red.svg", false),
    (".mdo", "seti-mdo-red.svg", false),
    (".cls", "seti-salesforce-blue.svg", false),
    (".apex", "seti-salesforce-blue.svg", false),
    (".asm", "seti-asm-red.svg", false),
    (".s", "seti-asm-red.svg", false),
    (".bicep", "seti-bicep-blue.svg", false),
    (".bzl", "seti-bazel-green.svg", false),
    (".bazel", "seti-bazel-green.svg", false),
    (".BUILD", "seti-bazel-green.svg", false),
    (".WORKSPACE", "seti-bazel-green.svg", false),
    (".bazelignore", "seti-bazel-green.svg", false),
    (".bazelversion", "seti-bazel-green.svg", false),
    (".c", "seti-c-blue.svg", false),
    (".h", "seti-c-purple.svg", false),
    (".m", "seti-c-yellow.svg", false),
    (".cs", "seti-c-sharp-blue.svg", false),
    (".cshtml", "seti-html-blue.svg", false),
    (".aspx", "seti-html-blue.svg", false),
    (".ascx", "seti-html-green.svg", false),
    (".asax", "seti-html-yellow.svg", false),
    (".master", "seti-html-yellow.svg", false),
    (".cc", "seti-cpp-blue.svg", false),
    (".cpp", "seti-cpp-blue.svg", false),
    (".cxx", "seti-cpp-blue.svg", false),
    (".c++", "seti-cpp-blue.svg", false),
    (".hh", "seti-cpp-purple.svg", false),
    (".hpp", "seti-cpp-purple.svg", false),
    (".hxx", "seti-cpp-purple.svg", false),
    (".h++", "seti-cpp-purple.svg", false),
    (".mm", "seti-cpp-yellow.svg", false),
    (".clj", "seti-clojure-green.svg", false),
    (".cljs", "seti-clojure-green.svg", false),
    (".cljc", "seti-clojure-green.svg", false),
    (".edn", "seti-clojure-blue.svg", false),
    (".cfc", "seti-coldfusion-blue.svg", false),
    (".cfm", "seti-coldfusion-blue.svg", false),
    (".coffee", "seti-coffee-yellow.svg", false),
    (".litcoffee", "seti-coffee-yellow.svg", false),
    (".config", "seti-config-grey-light.svg", false),
    (".cfg", "seti-config-grey-light.svg", false),
    (".conf", "seti-config-grey-light.svg", false),
    (".cr", "seti-crystal-white.svg", false),
    (".ecr", "seti-crystal_embedded-white.svg", false),
    (".slang", "seti-crystal_embedded-white.svg", false),
    (".cson", "seti-json-yellow.svg", false),
    (".css", "seti-css-blue.svg", false),
    (".css.map", "seti-css-blue.svg", false),
    (".sss", "seti-css-blue.svg", false),
    (".csv", "seti-csv-green.svg", false),
    (".xls", "seti-xls-green.svg", false),
    (".xlsx", "seti-xls-green.svg", false),
    (".cu", "seti-cu-green.svg", false),
    (".cuh", "seti-cu-purple.svg", false),
    (".hu", "seti-cu-purple.svg", false),
    (".cake", "seti-cake-red.svg", false),
    (".ctp", "seti-cake_php-red.svg", false),
    (".d", "seti-d-red.svg", false),
    (".doc", "seti-word-blue.svg", false),
    (".docx", "seti-word-blue.svg", false),
    (".ejs", "seti-ejs-yellow.svg", false),
    (".ex", "seti-elixir-purple.svg", false),
    (".exs", "seti-elixir_script-purple.svg", false),
    ("mix", "seti-hex-red.svg", true),
    (".elm", "seti-elm-blue.svg", false),
    (".ico", "seti-favicon-yellow.svg", false),
    (".fs", "seti-f-sharp-blue.svg", false),
    (".fsx", "seti-f-sharp-blue.svg", false),
    (".gitignore", "seti-git-ignore.svg", false),
    (".gitconfig", "seti-git-ignore.svg", false),
    (".gitkeep", "seti-git-ignore.svg", false),
    (".gitattributes", "seti-git-ignore.svg", false),
    (".gitmodules", "seti-git-ignore.svg", false),
    ("COMMIT_EDITMSG", "seti-git-ignore.svg", false),
    ("MERGE_MSG", "seti-git-ignore.svg", false),
    (".go", "seti-go2-blue.svg", false),
    (".slide", "seti-go-blue.svg", false),
    (".article", "seti-go-blue.svg", false),
    (".gd", "seti-godot-blue.svg", false),
    (".godot", "seti-godot-red.svg", false),
    (".tres", "seti-godot-yellow.svg", false),
    (".tscn", "seti-godot-purple.svg", false),
    (".gradle", "seti-gradle-blue.svg", false),
    (".groovy", "seti-grails-green.svg", false),
    (".gsp", "seti-grails-green.svg", false),
    (".gql", "seti-graphql-pink.svg", false),
    (".graphql", "seti-graphql-pink.svg", false),
    (".graphqls", "seti-graphql-pink.svg", false),
    (".hack", "seti-hacklang-orange.svg", false),
    (".haml", "seti-haml-red.svg", false),
    (".handlebars", "seti-mustache-orange.svg", false),
    (".hbs", "seti-mustache-orange.svg", false),
    (".hjs", "seti-mustache-orange.svg", false),
    (".hs", "seti-haskell-purple.svg", false),
    (".lhs", "seti-haskell-purple.svg", false),
    (".hx", "seti-haxe-orange.svg", false),
    (".hxs", "seti-haxe-yellow.svg", false),
    (".hxp", "seti-haxe-blue.svg", false),
    (".hxml", "seti-haxe-purple.svg", false),
    (".html", "seti-html-orange.svg", false),
    (".jade", "seti-jade-red.svg", false),
    (".java", "seti-java-red.svg", false),
    (".class", "seti-java-blue.svg", false),
    (".classpath", "seti-java-red.svg", false),
    (".properties", "seti-java-red.svg", false),
    (".js", "seti-javascript-yellow.svg", false),
    (".js.map", "seti-javascript-yellow.svg", false),
    (".cjs", "seti-javascript-yellow.svg", false),
    (".cjs.map", "seti-javascript-yellow.svg", false),
    (".mjs", "seti-javascript-yellow.svg", false),
    (".mjs.map", "seti-javascript-yellow.svg", false),
    (".spec.js", "seti-javascript-orange.svg", false),
    (".spec.cjs", "seti-javascript-orange.svg", false),
    (".spec.mjs", "seti-javascript-orange.svg", false),
    (".test.js", "seti-javascript-orange.svg", false),
    (".test.cjs", "seti-javascript-orange.svg", false),
    (".test.mjs", "seti-javascript-orange.svg", false),
    (".es", "seti-javascript-yellow.svg", false),
    (".es5", "seti-javascript-yellow.svg", false),
    (".es6", "seti-javascript-yellow.svg", false),
    (".es7", "seti-javascript-yellow.svg", false),
    (".jinja", "seti-jinja-red.svg", false),
    (".jinja2", "seti-jinja-red.svg", false),
    (".json", "seti-json-yellow.svg", false),
    (".jl", "seti-julia-purple.svg", false),
    ("karma.conf.js", "seti-karma-green.svg", false),
    ("karma.conf.cjs", "seti-karma-green.svg", false),
    ("karma.conf.mjs", "seti-karma-green.svg", false),
    ("karma.conf.coffee", "seti-karma-green.svg", false),
    (".kt", "seti-kotlin-orange.svg", false),
    (".kts", "seti-kotlin-orange.svg", false),
    (".dart", "seti-dart-blue.svg", false),
    (".less", "seti-less-blue.svg", false),
    (".liquid", "seti-liquid-green.svg", false),
    (".ls", "seti-livescript-blue.svg", false),
    (".lua", "seti-lua-blue.svg", false),
    (".markdown", "seti-markdown-blue.svg", false),
    (".md", "seti-markdown-blue.svg", false),
    (".argdown", "seti-argdown-blue.svg", false),
    (".ad", "seti-argdown-blue.svg", false),
    ("README.md", "seti-info-blue.svg", false),
    ("README.txt", "seti-info-blue.svg", false),
    ("README", "seti-info-blue.svg", false),
    ("CHANGELOG.md", "seti-clock-blue.svg", false),
    ("CHANGELOG.txt", "seti-clock-blue.svg", false),
    ("CHANGELOG", "seti-clock-blue.svg", false),
    ("CHANGES.md", "seti-clock-blue.svg", false),
    ("CHANGES.txt", "seti-clock-blue.svg", false),
    ("CHANGES", "seti-clock-blue.svg", false),
    ("VERSION.md", "seti-clock-blue.svg", false),
    ("VERSION.txt", "seti-clock-blue.svg", false),
    ("VERSION", "seti-clock-blue.svg", false),
    ("mvnw", "seti-maven-red.svg", false),
    ("pom.xml", "seti-maven-red.svg", false),
    (".mustache", "seti-mustache-orange.svg", false),
    (".stache", "seti-mustache-orange.svg", false),
    (".nim", "seti-nim-yellow.svg", false),
    (".nims", "seti-nim-yellow.svg", false),
    (".github-issues", "seti-github-white.svg", false),
    (".ipynb", "seti-notebook-blue.svg", false),
    (".njk", "seti-nunjucks-green.svg", false),
    (".nunjucks", "seti-nunjucks-green.svg", false),
    (".nunjs", "seti-nunjucks-green.svg", false),
    (".nunj", "seti-nunjucks-green.svg", false),
    (".njs", "seti-nunjucks-green.svg", false),
    (".nj", "seti-nunjucks-green.svg", false),
    (".npm-debug.log", "seti-npm-ignore.svg", false),
    (".npmignore", "seti-npm-red.svg", false),
    (".npmrc", "seti-npm-red.svg", false),
    (".ml", "seti-ocaml-orange.svg", false),
    (".mli", "seti-ocaml-orange.svg", false),
    (".cmx", "seti-ocaml-orange.svg", false),
    (".cmxa", "seti-ocaml-orange.svg", false),
    (".odata", "seti-odata-orange.svg", false),
    (".pl", "seti-perl-blue.svg", false),
    (".php", "seti-php-purple.svg", false),
    (".php.inc", "seti-php-purple.svg", false),
    (".pipeline", "seti-pipeline-orange.svg", false),
    (".pddl", "seti-pddl-purple.svg", false),
    (".plan", "seti-plan-green.svg", false),
    (".happenings", "seti-happenings-blue.svg", false),
    (".ps1", "seti-powershell-blue.svg", false),
    (".psd1", "seti-powershell-blue.svg", false),
    (".psm1", "seti-powershell-blue.svg", false),
    (".prisma", "seti-prisma-blue.svg", false),
    (".pug", "seti-pug-red.svg", false),
    (".pp", "seti-puppet-yellow.svg", false),
    (".epp", "seti-puppet-yellow.svg", false),
    (".purs", "seti-purescript-white.svg", false),
    (".py", "seti-python-blue.svg", false),
    (".jsx", "seti-react-blue.svg", false),
    (".spec.jsx", "seti-react-orange.svg", false),
    (".test.jsx", "seti-react-orange.svg", false),
    (".cjsx", "seti-react-blue.svg", false),
    (".tsx", "seti-react-blue.svg", false),
    (".spec.tsx", "seti-react-orange.svg", false),
    (".test.tsx", "seti-react-orange.svg", false),
    (".re", "seti-reasonml-red.svg", false),
    (".res", "seti-rescript-red.svg", false),
    (".resi", "seti-rescript-pink.svg", false),
    (".R", "seti-R-blue.svg", false),
    (".rmd", "seti-R-blue.svg", false),
    (".rb", "seti-ruby-red.svg", false),
    ("Gemfile", "seti-ruby-red.svg", true),
    ("gemfile", "seti-ruby-red.svg", true),
    (".erb", "seti-html_erb-red.svg", false),
    (".erb.html", "seti-html_erb-red.svg", false),
    (".html.erb", "seti-html_erb-red.svg", false),
    (".rs", "seti-rust-grey-light.svg", false),
    (".sass", "seti-sass-pink.svg", false),
    (".scss", "seti-sass-pink.svg", false),
    (".springBeans", "seti-spring-green.svg", false),
    (".slim", "seti-slim-orange.svg", false),
    (".smarty.tpl", "seti-smarty-yellow.svg", false),
    (".tpl", "seti-smarty-yellow.svg", false),
    (".sbt", "seti-sbt-blue.svg", false),
    (".scala", "seti-scala-red.svg", false),
    (".sol", "seti-ethereum-blue.svg", false),
    (".styl", "seti-stylus-green.svg", false),
    (".svelte", "seti-svelte-red.svg", false),
    (".swift", "seti-swift-orange.svg", false),
    (".sql", "seti-db-pink.svg", false),
    (".soql", "seti-db-blue.svg", false),
    (".tf", "seti-terraform-purple.svg", false),
    (".tf.json", "seti-terraform-purple.svg", false),
    (".tfvars", "seti-terraform-purple.svg", false),
    (".tfvars.json", "seti-terraform-purple.svg", false),
    (".tex", "seti-tex-blue.svg", false),
    (".sty", "seti-tex-yellow.svg", false),
    (".dtx", "seti-tex-orange.svg", false),
    (".ins", "seti-tex-white.svg", false),
    (".txt", "seti-default-white.svg", false),
    (".toml", "seti-config-grey-light.svg", false),
    (".twig", "seti-twig-green.svg", false),
    (".ts", "seti-typescript-blue.svg", false),
    (".spec.ts", "seti-typescript-orange.svg", false),
    (".test.ts", "seti-typescript-orange.svg", false),
    ("tsconfig.json", "seti-tsconfig-blue.svg", false),
    (".vala", "seti-vala-grey-light.svg", false),
    (".vapi", "seti-vala-grey-light.svg", false),
    (".component", "seti-html-orange.svg", false),
    ("vite.config.js", "seti-vite-yellow.svg", false),
    ("vite.config.ts", "seti-vite-yellow.svg", false),
    ("vite.config.mjs", "seti-vite-yellow.svg", false),
    ("vite.config.mts", "seti-vite-yellow.svg", false),
    ("vite.config.cjs", "seti-vite-yellow.svg", false),
    ("vite.config.cts", "seti-vite-yellow.svg", false),
    (".vue", "seti-vue-green.svg", false),
    (".wasm", "seti-wasm-purple.svg", false),
    (".wat", "seti-wat-purple.svg", false),
    (".xml", "seti-xml-orange.svg", false),
    (".yml", "seti-yml-purple.svg", false),
    (".yaml", "seti-yml-purple.svg", false),
    ("swagger.json", "seti-json-green.svg", false),
    ("swagger.yml", "seti-json-green.svg", false),
    ("swagger.yaml", "seti-json-green.svg", false),
    (".pro", "seti-prolog-orange.svg", false),
    (".zig", "seti-zig-orange.svg", false),
    (".jar", "seti-zip-red.svg", false),
    (".zip", "seti-zip-grey-light.svg", false),
    (".wgt", "seti-wgt-blue.svg", false),
    (".ai", "seti-illustrator-yellow.svg", false),
    (".psd", "seti-photoshop-blue.svg", false),
    (".pdf", "seti-pdf-red.svg", false),
    (".eot", "seti-font-red.svg", false),
    (".ttf", "seti-font-red.svg", false),
    (".woff", "seti-font-red.svg", false),
    (".woff2", "seti-font-red.svg", false),
    (".otf", "seti-font-red.svg", false),
    (".avif", "seti-image-purple.svg", false),
    (".gif", "seti-image-purple.svg", false),
    (".jpg", "seti-image-purple.svg", false),
    (".jpeg", "seti-image-purple.svg", false),
    (".png", "seti-image-purple.svg", false),
    (".pxm", "seti-image-purple.svg", false),
    (".svg", "seti-svg-purple.svg", false),
    (".svgx", "seti-image-purple.svg", false),
    (".tiff", "seti-image-purple.svg", false),
    (".webp", "seti-image-purple.svg", false),
    (".sublime-project", "seti-sublime-orange.svg", false),
    (".sublime-workspace", "seti-sublime-orange.svg", false),
    (".code-search", "seti-code-search-purple.svg", false),
    (".sh", "seti-shell-green.svg", false),
    (".zsh", "seti-shell-green.svg", false),
    (".fish", "seti-shell-green.svg", false),
    (".zshrc", "seti-shell-green.svg", false),
    (".bashrc", "seti-shell-green.svg", false),
    (".mov", "seti-video-pink.svg", false),
    (".ogv", "seti-video-pink.svg", false),
    (".webm", "seti-video-pink.svg", false),
    (".avi", "seti-video-pink.svg", false),
    (".mpg", "seti-video-pink.svg", false),
    (".mp4", "seti-video-pink.svg", false),
    (".mp3", "seti-audio-purple.svg", false),
    (".ogg", "seti-audio-purple.svg", false),
    (".wav", "seti-audio-purple.svg", false),
    (".flac", "seti-audio-purple.svg", false),
    (".3ds", "seti-svg-blue.svg", false),
    (".3dm", "seti-svg-blue.svg", false),
    (".stl", "seti-svg-blue.svg", false),
    (".obj", "seti-svg-blue.svg", false),
    (".dae", "seti-svg-blue.svg", false),
    (".bat", "seti-windows-blue.svg", false),
    (".cmd", "seti-windows-blue.svg", false),
    ("mime.types", "seti-config-grey-light.svg", false),
    ("Jenkinsfile", "seti-jenkins-red.svg", false),
    (".babelrc", "seti-babel-yellow.svg", false),
    (".babelrc.js", "seti-babel-yellow.svg", false),
    (".babelrc.cjs", "seti-babel-yellow.svg", false),
    ("babel.config.js", "seti-babel-yellow.svg", false),
    ("babel.config.json", "seti-babel-yellow.svg", false),
    ("babel.config.cjs", "seti-babel-yellow.svg", false),
    ("BUILD", "seti-bazel-green.svg", false),
    ("BUILD.bazel", "seti-bazel-green.svg", false),
    ("WORKSPACE", "seti-bazel-green.svg", false),
    ("WORKSPACE.bazel", "seti-bazel-green.svg", false),
    (".bazelrc", "seti-bazel-grey.svg", false),
    ("bower.json", "seti-bower-orange.svg", false),
    ("Bower.json", "seti-bower-orange.svg", false),
    (".bowerrc", "seti-bower-orange.svg", false),
    ("dockerfile", "seti-docker-blue.svg", true),
    ("Dockerfile", "seti-docker-blue.svg", true),
    ("DOCKERFILE", "seti-docker-blue.svg", true),
    (".dockerignore", "seti-docker-grey.svg", true),
    ("docker-healthcheck", "seti-docker-green.svg", true),
    ("docker-compose.yml", "seti-docker-pink.svg", true),
    ("docker-compose.yaml", "seti-docker-pink.svg", true),
    ("docker-compose.override.yml", "seti-docker-pink.svg", true),
    ("docker-compose.override.yaml", "seti-docker-pink.svg", true),
    (".codeclimate.yml", "seti-code-climate-green.svg", false),
    (".eslintrc", "seti-eslint-purple.svg", false),
    (".eslintrc.js", "seti-eslint-purple.svg", false),
    (".eslintrc.cjs", "seti-eslint-purple.svg", false),
    (".eslintrc.yaml", "seti-eslint-purple.svg", false),
    (".eslintrc.yml", "seti-eslint-purple.svg", false),
    (".eslintrc.json", "seti-eslint-purple.svg", false),
    (".eslintignore", "seti-eslint-grey.svg", false),
    ("eslint.config.js", "seti-eslint-purple.svg", false),
    (".firebaserc", "seti-firebase-orange.svg", false),
    ("firebase.json", "seti-firebase-orange.svg", false),
    ("geckodriver", "seti-firefox-orange.svg", false),
    (".gitlab-ci.yml", "seti-gitlab-orange.svg", false),
    ("Gruntfile.js", "seti-grunt-orange.svg", false),
    ("gruntfile.babel.js", "seti-grunt-orange.svg", false),
    ("Gruntfile.babel.js", "seti-grunt-orange.svg", false),
    ("gruntfile.js", "seti-grunt-orange.svg", false),
    ("Gruntfile.coffee", "seti-grunt-orange.svg", false),
    ("gruntfile.coffee", "seti-grunt-orange.svg", false),
    ("GULPFILE", "seti-gulp-red.svg", true),
    ("Gulpfile", "seti-gulp-red.svg", true),
    ("gulpfile", "seti-gulp-red.svg", true),
    ("gulpfile.js", "seti-gulp-red.svg", true),
    ("ionic.config.json", "seti-ionic-blue.svg", false),
    ("Ionic.config.json", "seti-ionic-blue.svg", false),
    ("ionic.project", "seti-ionic-blue.svg", false),
    ("Ionic.project", "seti-ionic-blue.svg", false),
    (".jshintrc", "seti-javascript-blue.svg", false),
    (".jscsrc", "seti-javascript-blue.svg", false),
    ("platformio.ini", "seti-platformio-orange.svg", false),
    ("rollup.config.js", "seti-rollup-red.svg", false),
    ("sass-lint.yml", "seti-sass-pink.svg", false),
    (".stylelintrc", "seti-stylelint-white.svg", false),
    (".stylelintrc.json", "seti-stylelint-white.svg", false),
    (".stylelintrc.yaml", "seti-stylelint-white.svg", false),
    (".stylelintrc.yml", "seti-stylelint-white.svg", false),
    (".stylelintrc.js", "seti-stylelint-white.svg", false),
    (".stylelintignore", "seti-stylelint-grey.svg", false),
    ("stylelint.config.js", "seti-stylelint-white.svg", false),
    ("stylelint.config.cjs", "seti-stylelint-white.svg", false),
    ("stylelint.config.mjs", "seti-stylelint-white.svg", false),
    ("yarn.clean", "seti-yarn-blue.svg", false),
    ("yarn.lock", "seti-yarn-blue.svg", false),
    ("webpack.config.js", "seti-webpack-blue.svg", false),
    ("webpack.config.cjs", "seti-webpack-blue.svg", false),
    ("webpack.config.mjs", "seti-webpack-blue.svg", false),
    ("webpack.config.ts", "seti-webpack-blue.svg", false),
    ("webpack.config.build.js", "seti-webpack-blue.svg", false),
    ("webpack.config.build.cjs", "seti-webpack-blue.svg", false),
    ("webpack.config.build.mjs", "seti-webpack-blue.svg", false),
    ("webpack.config.build.ts", "seti-webpack-blue.svg", false),
    ("webpack.common.js", "seti-webpack-blue.svg", false),
    ("webpack.common.cjs", "seti-webpack-blue.svg", false),
    ("webpack.common.mjs", "seti-webpack-blue.svg", false),
    ("webpack.common.ts", "seti-webpack-blue.svg", false),
    ("webpack.dev.js", "seti-webpack-blue.svg", false),
    ("webpack.dev.cjs", "seti-webpack-blue.svg", false),
    ("webpack.dev.mjs", "seti-webpack-blue.svg", false),
    ("webpack.dev.ts", "seti-webpack-blue.svg", false),
    ("webpack.prod.js", "seti-webpack-blue.svg", false),
    ("webpack.prod.cjs", "seti-webpack-blue.svg", false),
    ("webpack.prod.mjs", "seti-webpack-blue.svg", false),
    ("webpack.prod.ts", "seti-webpack-blue.svg", false),
    (".direnv", "seti-config-grey-light.svg", false),
    (".env", "seti-config-grey-light.svg", false),
    (".static", "seti-config-grey-light.svg", false),
    (".editorconfig", "seti-config-grey-light.svg", false),
    (".slugignore", "seti-config-grey-light.svg", false),
    (".tmp", "seti-clock-grey-light.svg", false),
    (".htaccess", "seti-config-grey-light.svg", false),
    (".key", "seti-lock-green.svg", false),
    (".cert", "seti-lock-green.svg", false),
    (".cer", "seti-lock-green.svg", false),
    (".crt", "seti-lock-green.svg", false),
    (".pem", "seti-lock-green.svg", false),
    ("LICENSE", "seti-license-yellow.svg", true),
    ("LICENCE", "seti-license-yellow.svg", true),
    ("LICENSE.txt", "seti-license-yellow.svg", true),
    ("LICENCE.txt", "seti-license-yellow.svg", true),
    ("LICENSE.md", "seti-license-yellow.svg", true),
    ("LICENCE.md", "seti-license-yellow.svg", true),
    ("COPYING", "seti-license-yellow.svg", true),
    ("COPYING.txt", "seti-license-yellow.svg", true),
    ("COPYING.md", "seti-license-yellow.svg", true),
    ("COMPILING", "seti-license-orange.svg", true),
    ("COMPILING.txt", "seti-license-orange.svg", true),
    ("COMPILING.md", "seti-license-orange.svg", true),
    ("CONTRIBUTING", "seti-license-red.svg", true),
    ("CONTRIBUTING.txt", "seti-license-red.svg", true),
    ("CONTRIBUTING.md", "seti-license-red.svg", true),
    ("MAKEFILE", "seti-makefile-orange.svg", true),
    ("Makefile", "seti-makefile-orange.svg", true),
    ("makefile", "seti-makefile-orange.svg", true),
    ("QMAKEFILE", "seti-makefile-purple.svg", true),
    ("QMakefile", "seti-makefile-purple.svg", true),
    ("qmakefile", "seti-makefile-purple.svg", true),
    ("OMAKEFILE", "seti-makefile-grey-light.svg", true),
    ("OMakefile", "seti-makefile-grey-light.svg", true),
    ("omakefile", "seti-makefile-grey-light.svg", true),
    ("CMAKELISTS.TXT", "seti-makefile-blue.svg", true),
    ("CMAKELISTS.txt", "seti-makefile-blue.svg", true),
    ("CMakeLists.txt", "seti-makefile-blue.svg", true),
    ("cmakelists.txt", "seti-makefile-blue.svg", true),
    ("Procfile", "seti-heroku-purple.svg", true),
    ("TODO", "seti-todo-seti-primary.svg", true),
    ("TODO.txt", "seti-todo-seti-primary.svg", true),
    ("TODO.md", "seti-todo-seti-primary.svg", true),
    ("npm-debug.log", "seti-npm_ignored-ignore.svg", false),
    (".DS_Store", "seti-ignored-ignore.svg", false),
];

// embedded payloads: macro invocation (stems literal => include! paths)
macro_rules! seti_switch {
    [$($stem:literal),* $(,)?] => {
        pub(crate) fn svg_data(name: &str) -> Option<&'static [u8]> {
            name.strip_prefix("seti-")
                .and_then(|n| n.strip_suffix(".svg"))
                .and_then(|stem| match stem {
                    $($stem => Some(&include_bytes!(concat!(
                        env!("CARGO_MANIFEST_DIR"), "/assets/icons/seti-", $stem, ".svg"))[..]),)*
                    _ => None,
                })
        }

        #[cfg(test)]
        pub(crate) const STEMS: &[&str] = &[$($stem),*];
    };
}

seti_switch![
    "R-blue",
    "argdown-blue",
    "asm-red",
    "audio-purple",
    "babel-yellow",
    "bazel-green",
    "bazel-grey",
    "bicep-blue",
    "bower-orange",
    "bsl-red",
    "c-blue",
    "c-purple",
    "c-sharp-blue",
    "c-yellow",
    "cake-red",
    "cake_php-red",
    "clock-blue",
    "clock-grey-light",
    "clojure-blue",
    "clojure-green",
    "code-climate-green",
    "code-search-purple",
    "coffee-yellow",
    "coldfusion-blue",
    "config-grey-light",
    "cpp-blue",
    "cpp-purple",
    "cpp-yellow",
    "crystal-white",
    "crystal_embedded-white",
    "css-blue",
    "csv-green",
    "cu-green",
    "cu-purple",
    "d-red",
    "dart-blue",
    "db-blue",
    "db-pink",
    "default-white",
    "docker-blue",
    "docker-green",
    "docker-grey",
    "docker-pink",
    "ejs-yellow",
    "elixir-purple",
    "elixir_script-purple",
    "elm-blue",
    "eslint-grey",
    "eslint-purple",
    "ethereum-blue",
    "f-sharp-blue",
    "favicon-yellow",
    "firebase-orange",
    "firefox-orange",
    "font-red",
    "git-ignore",
    "github-white",
    "gitlab-orange",
    "go-blue",
    "go2-blue",
    "godot-blue",
    "godot-purple",
    "godot-red",
    "godot-yellow",
    "gradle-blue",
    "grails-green",
    "graphql-pink",
    "grunt-orange",
    "gulp-red",
    "hacklang-orange",
    "haml-red",
    "happenings-blue",
    "haskell-purple",
    "haxe-blue",
    "haxe-orange",
    "haxe-purple",
    "haxe-yellow",
    "heroku-purple",
    "hex-red",
    "html-blue",
    "html-green",
    "html-orange",
    "html-yellow",
    "html_erb-red",
    "ignored-ignore",
    "illustrator-yellow",
    "image-purple",
    "info-blue",
    "ionic-blue",
    "jade-red",
    "java-blue",
    "java-red",
    "javascript-blue",
    "javascript-orange",
    "javascript-yellow",
    "jenkins-red",
    "jinja-red",
    "json-green",
    "json-yellow",
    "julia-purple",
    "karma-green",
    "kotlin-orange",
    "less-blue",
    "license-orange",
    "license-red",
    "license-yellow",
    "liquid-green",
    "livescript-blue",
    "lock-green",
    "lua-blue",
    "makefile-blue",
    "makefile-grey-light",
    "makefile-orange",
    "makefile-purple",
    "markdown-blue",
    "maven-red",
    "mdo-red",
    "mustache-orange",
    "nim-yellow",
    "notebook-blue",
    "npm-ignore",
    "npm-red",
    "npm_ignored-ignore",
    "nunjucks-green",
    "ocaml-orange",
    "odata-orange",
    "pddl-purple",
    "pdf-red",
    "perl-blue",
    "photoshop-blue",
    "php-purple",
    "pipeline-orange",
    "plan-green",
    "platformio-orange",
    "powershell-blue",
    "prisma-blue",
    "prolog-orange",
    "pug-red",
    "puppet-yellow",
    "purescript-white",
    "python-blue",
    "react-blue",
    "react-orange",
    "reasonml-red",
    "rescript-pink",
    "rescript-red",
    "rollup-red",
    "ruby-red",
    "rust-grey-light",
    "salesforce-blue",
    "sass-pink",
    "sbt-blue",
    "scala-red",
    "shell-green",
    "slim-orange",
    "smarty-yellow",
    "spring-green",
    "stylelint-grey",
    "stylelint-white",
    "stylus-green",
    "sublime-orange",
    "svelte-red",
    "svg-blue",
    "svg-purple",
    "swift-orange",
    "terraform-purple",
    "tex-blue",
    "tex-orange",
    "tex-white",
    "tex-yellow",
    "todo-seti-primary",
    "tsconfig-blue",
    "twig-green",
    "typescript-blue",
    "typescript-orange",
    "vala-grey-light",
    "video-pink",
    "vite-yellow",
    "vue-green",
    "wasm-purple",
    "wat-purple",
    "webpack-blue",
    "wgt-blue",
    "windows-blue",
    "word-blue",
    "xls-green",
    "xml-orange",
    "yarn-blue",
    "yml-purple",
    "zig-orange",
    "zip-grey-light",
    "zip-red",
];
/// seti mapping.less render law: every rule shares one specificity, so the
/// LAST match wins (defaults sit first, specifics later — that layering is
/// what gives "spec.js" its orange variant and "tsconfig.json" its own glyph).
/// Ext patterns (leading `.`) compare case-insensitively (dev trees: MAIN.PY
/// gets the .py glyph); other patterns match as authored (BUILD ≠ build);
/// `partial` = substring (Makefile inside a suffixed name).
pub(crate) fn icon_for(name: &str) -> &'static str {
    let mut hit = "";
    for (pat, file, partial) in RULES {
        let m = if *partial {
            name.contains(pat)
        } else if pat.starts_with('.') {
            ends_with_ci(name, pat)
        } else {
            name.ends_with(pat) || pat.is_empty()
        };
        if m {
            hit = file;
        }
    }
    hit
}

/// Case-insensitive ends_with; needles are ASCII (generated art names), so the
/// only boundary risk is splitting a multi-byte char in the haystack tail.
fn ends_with_ci(hay: &str, needle: &str) -> bool {
    let n = needle.len();
    hay.len() >= n
        && hay.is_char_boundary(hay.len() - n)
        && hay[hay.len() - n..].eq_ignore_ascii_case(needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_resolves_per_seti_cascade() {
        assert_eq!(icon_for("app.py"), "seti-python-blue.svg");
        assert_eq!(icon_for("MAIN.PY"), "seti-python-blue.svg"); // ext rules are ci
        assert_eq!(icon_for("a.spec.js"), "seti-javascript-orange.svg"); // later rule beats .js
        assert_eq!(icon_for("tsconfig.json"), "seti-tsconfig-blue.svg"); // name rule beats .json
        assert_eq!(icon_for("Makefile"), "seti-makefile-orange.svg"); // partial, authored case
        assert_eq!(icon_for("docker-compose.yml"), "seti-docker-pink.svg"); // partial beats .yml
        assert_eq!(icon_for(".gitignore"), "seti-git-ignore.svg");
        assert_eq!(icon_for("LICENSE.md"), "seti-license-yellow.svg");
        assert_eq!(icon_for("main.rs"), "seti-rust-grey-light.svg");
        assert_eq!(icon_for(".DS_Store"), "seti-ignored-ignore.svg"); // junk renders dimmed
        assert_eq!(icon_for("a.R"), "seti-R-blue.svg");
        assert_eq!(icon_for("build"), "seti-default-white.svg"); // BUILD is authored-case
    }

    #[test]
    fn every_seti_asset_rasterizes_16() {
        // the bake (color/class/style strips + monochrome law) must leave each
        // file rasterizable by the system svg loader into a full 16px cell
        for stem in STEMS {
            let name = format!("seti-{stem}.svg");
            let pix = crate::badges::svg_pixbuf(&name, 16)
                .unwrap_or_else(|| panic!("{name}: rasterize failed"));
            assert_eq!(pix.width(), 16, "{name}");
        }
    }
}