# nera Web Framework: A Django-Shaped Approach to Erlang Web Development

> *"Erlang doesn't need Rails. It needs Django — explicit, batteries-included,
> and unapologetically pragmatic."*

## 1. Why Erlang Web Frameworks Failed

Between 2008 and 2016, the Erlang community produced several web frameworks:
**Chicago Boss** (Rails-like MVC), **Nitrogen** (widget-based), **BeepBeep**,
**Zotonic** (CMS), and others. All faded. Meanwhile, Elixir's Phoenix thrived.

The failures share common causes:

| Root cause            | Detail                                                                                                                                                                          |
|-----------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **Syntax barrier**    | Erlang's syntax scares web developers before they see any benefit. Elixir solved this with Ruby-like surface syntax — a cosmetic change with massive adoption impact.           |
| **String pain**       | Web development is 80% string manipulation. `"hello"` = `[104,101,108,108,111]` is a non-starter for HTML/JSON/URL work.                                                        |
| **No macro DSLs**     | Phoenix's `get "/users", UserController, :index` is an Elixir macro. Erlang has parse transforms — clunky and rarely used. You can't build pleasant routing DSLs the Rails way. |
| **Tooling gap**       | Mix + Hex gave Elixir a complete project lifecycle from day one. Rebar3 came late and never matched the developer experience.                                                   |
| **Cultural mismatch** | Erlang developers build telecom switches and message brokers. The community self-selected away from "make a blog in 15 minutes" demos.                                          |
| **Incomplete stacks** | Every framework provided routing but left auth, sessions, templating, data access, and admin as exercises for the reader. Web developers need the full stack or they leave.     |

The key lesson: **Phoenix succeeded not because of the BEAM, but because José
Valim understood developer experience.** The Erlang attempts failed not on
technical merits but on ergonomics and completeness.

---

## 2. Why Django, Not Rails

Rails and Phoenix are "magical" — heavy metaprogramming, implicit conventions,
DSLs built on macros. Django takes a different path:

| Principle           | Django                                                  | Rails/Phoenix                              |
|---------------------|---------------------------------------------------------|--------------------------------------------|
| **Philosophy**      | Explicit over implicit                                  | Convention over configuration              |
| **Metaprogramming** | Minimal — plain Python functions                        | Extensive — macros, method_missing, mixins |
| **Completeness**    | Batteries included: auth, admin, ORM, forms, middleware | Core is thin, plugins for everything       |
| **Learning curve**  | Read the code, understand the code                      | Learn the magic, then debug the magic      |

This maps naturally to Erlang:

- Erlang is **explicit** — you see what's happening, there's no hidden dispatch.
- Erlang **lacks macros** — and that's fine if you don't need them.
- Erlang's **pattern matching IS the router** — no DSL required.
- Erlang's **process model** replaces middleware stacks with simple pipelines.
- Erlang's **hot code reload** is a genuine advantage over Django's file-watcher.

An Erlang web framework shouldn't try to be Phoenix. It should be
**Django-shaped**: opinionated, complete, explicit, and focused on the developer
doing real work rather than admiring the architecture.

---

## 3. What nera Already Provides

nera is not starting from zero. The compiler and runtime already have pieces
that a web framework builds on.

### Available today

| Capability                                    | Status    | Relevance                                          |
|-----------------------------------------------|-----------|----------------------------------------------------|
| `gen_tcp` (listen, accept, recv, send, close) | ✅ Working | HTTP server foundation                             |
| Async I/O pool                                | ✅ Working | Non-blocking socket ops, process suspension/resume |
| Process-per-connection                        | ✅ Working | Natural HTTP handler isolation                     |
| Pattern matching on binaries                  | ✅ Working | HTTP request parsing                               |
| Maps                                          | ✅ Working | Request/response data structures                   |
| Closures                                      | ✅ Working | Middleware composition                             |
| Preemptive scheduling                         | ✅ Working | Fair request handling under load                   |
| Process links & monitors                      | ✅ Working | Handler crash isolation                            |
| Registered processes                          | ✅ Working | Named services (router, session store)             |
| Multi-module compilation                      | ✅ Working | Framework + app code linked together               |
| Records                                       | ✅ Working | Structured data (models)                           |

### Planned / in progress

| Capability                                  | Status                         | Required for                         |
|---------------------------------------------|--------------------------------|--------------------------------------|
| Binary strings (`-compile(binary_strings)`) | Designed (STRING_MIGRATION.md) | Sane string handling everywhere      |
| String interpolation (`~"Hello, ${Name}!"`) | Designed                       | Template rendering                   |
| `gen_server` behaviour                      | Not yet                        | Stateful services (sessions, caches) |
| `supervisor` behaviour                      | Not yet                        | Production reliability               |
| ETS (in-memory tables)                      | Not yet                        | Session store, routing table cache   |
| `file` module                               | Not yet                        | Template loading, static files       |
| `json` module                               | Not yet                        | API responses                        |
| `crypto` / `base64`                         | Not yet                        | Session tokens, auth                 |

---

## 4. Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        User Application                         │
│  myapp_handler.erl   myapp_models.erl   templates/*.html        │
├─────────────────────────────────────────────────────────────────┤
│                     nera_web Framework                          │
│                                                                 │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐            │
│  │  Router  │ │ Template │ │   Data   │ │  Admin   │            │
│  │ (pattern │ │  Engine  │ │  Layer   │ │Generator │            │
│  │  match)  │ │(compiled)│ │(Mnesia/  │ │  (CRUD)  │            │
│  └──────────┘ └──────────┘ │  ETS)    │ └──────────┘            │
│  ┌──────────┐ ┌──────────┐ └──────────┘  ┌──────────┐           │
│  │Middleware│ │ Sessions │ ┌──────────┐  │  Static  │           │
│  │ Pipeline │ │  & Auth  │ │  Forms   │  │  Files   │           │
│  └──────────┘ └──────────┘ └──────────┘  └──────────┘           │
├─────────────────────────────────────────────────────────────────┤
│                    nera_http Library                            │
│  HTTP/1.1 parser · Request/Response records · Status codes      │
├─────────────────────────────────────────────────────────────────┤
│                     nera Runtime                                │
│  gen_tcp · Async I/O · Processes · Scheduling · GC · Heap       │
└─────────────────────────────────────────────────────────────────┘
```

Two stdlib layers, following nera's three-tier architecture (STDLIB_DESIGN.md):

- **nera_http** — Tier 3 (pure Erlang): HTTP parsing, request/response records,
  status codes, header manipulation. Compiled by nera alongside user code.
- **nera_web** — Tier 3 (pure Erlang): the full framework. Router, templates,
  data layer, middleware, sessions, admin. Depends on nera_http.

Heavy lifting (JSON encoding, crypto hashing, regex) lives in Tier 2 (Rust
extensions) as FFI calls — same pattern as `lists:reverse/1` today.

---

## 5. Core Components

### 5.1 HTTP Server

A process-per-connection HTTP/1.1 server built on `gen_tcp`. No external
dependencies.

```erlang
-module(nera_http_server).
-export([start/2]).

start(Port, Handler) ->
    {ok, LSock} = gen_tcp:listen(Port, []),
    io:format("Listening on port ~w~n", [Port]),
    accept_loop(LSock, Handler).

accept_loop(LSock, Handler) ->
    {ok, Sock} = gen_tcp:accept(LSock),
    spawn(fun() -> handle_connection(Sock, Handler) end),
    accept_loop(LSock, Handler).

handle_connection(Sock, Handler) ->
    {ok, Data} = gen_tcp:recv(Sock, 0),
    Request = nera_http:parse_request(Data),
    Response = Handler(Request),
    gen_tcp:send(Sock, nera_http:encode_response(Response)),
    gen_tcp:close(Sock).
```

The HTTP parser is a binary pattern-matching function — Erlang's natural
strength:

```erlang
-module(nera_http).
-export([parse_request/1]).

parse_request(Data) ->
    {Method, Rest1} = parse_method(Data),
    {Path, Rest2} = parse_path(Rest1),
    {Headers, Body} = parse_headers(Rest2),
    #{method => Method,
      path => split_path(Path),
      raw_path => Path,
      headers => Headers,
      body => Body,
      params => #{}}.

parse_method(<<$G, $E, $T, $\s, Rest/binary>>) -> {get, Rest};
parse_method(<<$P, $O, $S, $T, $\s, Rest/binary>>) -> {post, Rest};
parse_method(<<$P, $U, $T, $\s, Rest/binary>>) -> {put, Rest};
parse_method(<<$D, $E, $L, $E, $T, $E, $\s, Rest/binary>>) -> {delete, Rest}.
```

### 5.2 Router — Pattern Matching IS the DSL

This is where Erlang's nature pays off. No macros, no DSL, no magic.
**Function clauses ARE routes.**

```erlang
-module(myapp_handler).
-export([handle/1]).

%% Route: GET /
handle(#{method := get, path := []}) ->
    nera_web:render("index.html", #{title => "Home"});

%% Route: GET /users
handle(#{method := get, path := [<<"users">>]}) ->
    Users = myapp_db:all(users),
    nera_web:render("users/index.html", #{users => Users});

%% Route: GET /users/:id
handle(#{method := get, path := [<<"users">>, Id]}) ->
    User = myapp_db:get(users, Id),
    nera_web:render("users/show.html", #{user => User});

%% Route: POST /users
handle(#{method := post, path := [<<"users">>]} = Req) ->
    Params = nera_web:parse_body(Req),
    myapp_db:insert(users, Params),
    nera_web:redirect("/users");

%% Route: DELETE /users/:id
handle(#{method := delete, path := [<<"users">>, Id]}) ->
    myapp_db:delete(users, Id),
    nera_web:redirect("/users");

%% 404 catch-all
handle(_Req) ->
    nera_web:not_found().
```

**Why this works:**

- Each clause is a route. The pattern IS the URL spec.
- Variable segments (`Id`) are captured by pattern matching — no regex, no parsing.
- The catch-all clause is a natural 404 handler.
- Adding a route = adding a function clause. Removing a route = removing a clause.
- The compiler verifies exhaustiveness and warns about unreachable clauses.
- Reading the handler IS reading the route table. No separate config file.

**Route composition** for larger apps — delegate by prefix:

```erlang
-module(myapp_handler).

handle(#{path := [<<"api">> | Rest]} = Req) ->
    myapp_api:handle(Req#{path := Rest});
handle(#{path := [<<"admin">> | Rest]} = Req) ->
    myapp_admin:handle(Req#{path := Rest});
handle(Req) ->
    myapp_pages:handle(Req).
```

This is Django's URL `include()` pattern — composed by nesting, not by
framework magic.

### 5.3 Request and Response

Requests and responses are plain maps. No opaque objects, no method calls.

```erlang
%% Request (produced by nera_http:parse_request/1)
#{method => get,
  path => [<<"users">>, <<"42">>],
  raw_path => <<"/users/42">>,
  headers => #{<<"content-type">> => <<"application/json">>,
               <<"host">> => <<"localhost:8080">>},
  body => <<>>,
  params => #{},          %% populated by middleware (query params, form data)
  session => #{}          %% populated by session middleware
}.

%% Response (returned by handlers)
#{status => 200,
  headers => #{<<"content-type">> => <<"text/html">>},
  body => <<"<html>...</html>">>
}.
```

Convenience constructors:

```erlang
nera_web:html(200, Body)               %% HTML response
nera_web:json(200, Data)               %% JSON response (auto-encodes map)
nera_web:redirect(Path)                %% 302 redirect
nera_web:not_found()                   %% 404
nera_web:render(Template, Vars)        %% Render template, return HTML response
```

### 5.4 Middleware Pipeline

Django-style middleware: a list of functions, each wrapping the next. No
special protocol, just `fun(Request, Next) -> Response`.

```erlang
-module(myapp).
-export([start/0]).

start() ->
    Middlewares = [
        fun nera_web_logger:middleware/2,
        fun nera_web_static:middleware/2,
        fun nera_web_session:middleware/2,
        fun nera_web_csrf:middleware/2
    ],
    Handler = nera_web:wrap(fun myapp_handler:handle/1, Middlewares),
    nera_http_server:start(8080, Handler).
```

A middleware implementation:

```erlang
-module(nera_web_logger).
-export([middleware/2]).

middleware(Request, Next) ->
    #{method := Method, raw_path := Path} = Request,
    T0 = erlang:monotonic_time(millisecond),
    Response = Next(Request),
    T1 = erlang:monotonic_time(millisecond),
    #{status := Status} = Response,
    io:format("[~w] ~s ~s ~wms~n", [Status, Method, Path, T1 - T0]),
    Response.
```

The `wrap/2` function composes middlewares into a single handler:

```erlang
wrap(Handler, []) ->
    Handler;
wrap(Handler, [Mw | Rest]) ->
    Wrapped = wrap(Handler, Rest),
    fun(Req) -> Mw(Req, Wrapped) end.
```

That's the entire middleware engine. Three lines.

### 5.5 Template Engine

Compiled templates — like Django's template loader but producing Erlang modules.
A template file is parsed once at compile time into a function that takes a
variable map and returns a binary.

**Template syntax** — deliberately simple, Django-inspired:

```html
<!-- templates/users/index.html -->
<h1>{{ title }}</h1>
<ul>
{% for user in users %}
    <li><a href="/users/{{ user.id }}">{{ user.name }}</a></li>
{% endfor %}
</ul>

{% if admin %}
    <a href="/admin/users">Manage Users</a>
{% endif %}
```

**Compiles to** (conceptually):

```erlang
-module('templates/users/index.html').
-export([render/1]).

render(Vars) ->
    Title = maps:get(title, Vars, <<>>),
    Users = maps:get(users, Vars, []),
    Admin = maps:get(admin, Vars, false),
    iolist_to_binary([
        <<"<h1>">>, html_escape(Title), <<"</h1>\n<ul>\n">>,
        [begin
            Id = maps:get(id, User, <<>>),
            Name = maps:get(name, User, <<>>),
            [<<"    <li><a href=\"/users/">>, html_escape(Id),
             <<"\">">>, html_escape(Name), <<"</a></li>\n">>]
         end || User <- Users],
        <<"</ul>\n">>,
        case Admin of
            true -> <<"    <a href=\"/admin/users\">Manage Users</a>\n">>;
            _ -> <<>>
        end
    ]).
```

Key design choices:

- **Auto-escaping by default** — all `{{ var }}` output is HTML-escaped. Use
  `{{ var | raw }}` for unescaped output. (Django's approach.)
- **Limited logic** — `if`/`for`/`include` only. No arbitrary Erlang in
  templates. Templates are for presentation, not business logic.
- **Compiled** — templates are parsed at compile time by a nera preprocessor
  step and emitted as Erlang modules. Zero runtime parsing cost.
- **iolist output** — returns iolists (nested lists of binaries), which
  `gen_tcp:send/2` already accepts. No final concatenation needed.

### 5.6 Data Layer

Django's ORM is its most powerful feature. Erlang doesn't need an ORM — it
needs a **data access API** that's pleasant to use. The data layer wraps
Mnesia (or ETS for simpler cases) with a Django QuerySet-inspired interface.

**Model definition** — using Erlang records:

```erlang
-module(myapp_models).
-export([install/0]).

-record(user, {id, name, email, role = member, created_at}).
-record(article, {id, title, body, author_id, published = false}).

install() ->
    nera_db:create_table(user, record_info(fields, user), []),
    nera_db:create_table(article, record_info(fields, article), []).
```

**Query API** — functional, chainable:

```erlang
%% Get all users
nera_db:all(user)
%% => [#{id => 1, name => <<"Alice">>, ...}, ...]

%% Filter
nera_db:filter(user, #{role => admin})

%% Get one by ID
nera_db:get(user, 42)
%% => #{id => 42, name => <<"Bob">>, ...}   or   not_found

%% Insert
nera_db:insert(user, #{name => <<"Carol">>, email => <<"carol@ex.com">>})
%% => {ok, #{id => 3, ...}}

%% Update
nera_db:update(user, 42, #{name => <<"Robert">>})

%% Delete
nera_db:delete(user, 42)

%% Count
nera_db:count(user, #{role => admin})

%% Filter with comparison operators
nera_db:filter(article, #{published => true, author_id => 42})
```

Under the hood, this uses Mnesia transactions for writes and `mnesia:select/2`
with match specifications for reads. The map-based filter API is compiled to
Mnesia match specs at call time.

For simple apps that don't need persistence across restarts, `nera_db` can
back onto ETS instead of Mnesia — same API, simpler setup.

### 5.7 Sessions and Authentication

**Sessions** — stored server-side (ETS or Mnesia), identified by a cookie token.

```erlang
%% Session middleware reads/writes a session cookie
%% and populates Request#{session => SessionData}

%% In a handler:
handle(#{method := post, path := [<<"login">>]} = Req) ->
    #{params := #{<<"email">> := Email, <<"password">> := Pass}} = Req,
    case myapp_auth:verify(Email, Pass) of
        {ok, User} ->
            Session = #{user_id => maps:get(id, User)},
            nera_web:redirect("/dashboard", #{session => Session});
        error ->
            nera_web:render("login.html", #{error => <<"Invalid credentials">>})
    end;

handle(#{method := get, path := [<<"dashboard">>]} = Req) ->
    case maps:get(user_id, maps:get(session, Req, #{}), undefined) of
        undefined ->
            nera_web:redirect("/login");
        UserId ->
            User = nera_db:get(user, UserId),
            nera_web:render("dashboard.html", #{user => User})
    end.
```

**Auth helper** — a middleware that enforces login:

```erlang
require_auth(Req, Next) ->
    case maps:get(user_id, maps:get(session, Req, #{}), undefined) of
        undefined -> nera_web:redirect("/login");
        _UserId -> Next(Req)
    end.
```

### 5.8 Admin Interface Generator

Django's most celebrated feature: given model definitions, automatically
generate CRUD admin pages. This is feasible in Erlang using `record_info` and
the template engine.

```erlang
%% In myapp.erl:
start() ->
    nera_admin:register(user, record_info(fields, user), [
        {list_display, [name, email, role]},
        {search_fields, [name, email]}
    ]),
    nera_admin:register(article, record_info(fields, article), [
        {list_display, [title, published, author_id]},
        {search_fields, [title]}
    ]),
    %% Admin routes are auto-generated under /admin/
    ...
```

The admin generator produces:

- **List view**: paginated table with search and sort.
- **Detail view**: form with all fields, type-aware input widgets.
- **Create/edit/delete**: standard CRUD operations with confirmation.

All rendered using the template engine with built-in admin templates. No code
generation — the admin is a framework module that reads model metadata at
startup and dispatches accordingly.

### 5.9 Static Files

A middleware that serves files from a `static/` directory:

```erlang
-module(nera_web_static).
-export([middleware/2]).

middleware(#{path := [<<"static">> | FilePath]} = _Req, _Next) ->
    FullPath = filename:join(["static" | FilePath]),
    case file:read_file(FullPath) of
        {ok, Content} ->
            ContentType = nera_http:guess_content_type(FullPath),
            #{status => 200,
              headers => #{<<"content-type">> => ContentType},
              body => Content};
        {error, _} ->
            nera_web:not_found()
    end;
middleware(Req, Next) ->
    Next(Req).
```

For production, static files should be served by a reverse proxy. This
middleware is for development convenience only.

### 5.10 Forms and Validation

Django's forms system — define expected fields and validation rules, get
parsing + error messages for free:

```erlang
-module(myapp_forms).
-export([user_form/0]).

user_form() ->
    nera_form:new([
        {name, [{required, true}, {max_length, 100}]},
        {email, [{required, true}, {type, email}]},
        {role, [{choices, [member, admin]}, {default, member}]}
    ]).
```

Usage in a handler:

```erlang
handle(#{method := post, path := [<<"users">>]} = Req) ->
    Form = myapp_forms:user_form(),
    case nera_form:validate(Form, nera_web:parse_body(Req)) of
        {ok, CleanData} ->
            nera_db:insert(user, CleanData),
            nera_web:redirect("/users");
        {error, Errors} ->
            nera_web:render("users/new.html", #{errors => Errors})
    end.
```

---

## 6. Project Structure and Conventions

A nera_web project follows a fixed layout — convention, not configuration:

```
myapp/
├── src/
│   ├── myapp.erl              # Application entry point, starts server
│   ├── myapp_handler.erl      # Main route handler
│   ├── myapp_models.erl       # Data model definitions (-record)
│   ├── myapp_forms.erl        # Form definitions and validation
│   ├── myapp_auth.erl         # Authentication logic
│   ├── api/
│   │   └── myapp_api.erl      # JSON API handlers (delegated from main handler)
│   └── admin/
│       └── myapp_admin.erl    # Admin customization
├── templates/
│   ├── base.html              # Base layout (block inheritance)
│   ├── index.html
│   └── users/
│       ├── index.html
│       ├── show.html
│       └── new.html
├── static/
│   ├── css/
│   ├── js/
│   └── images/
└── nera.config                # Port, database, middleware list
```

**Scaffolding** via a `nera new` command:

```
$ nera new myapp
Created myapp/
  src/myapp.erl
  src/myapp_handler.erl
  templates/index.html
  static/css/style.css
  nera.config

$ cd myapp
$ nera run
Listening on port 8080
```

This is the "blog in 5 minutes" experience that Erlang has never had.

---

## 7. Configuration

A single `nera.config` file — Erlang terms, not YAML/TOML:

```erlang
#{
    port => 8080,
    secret_key => <<"change-me-in-production">>,
    database => #{
        backend => ets,     %% ets | mnesia
        persist => false
    },
    middlewares => [
        nera_web_logger,
        nera_web_static,
        nera_web_session,
        nera_web_csrf
    ],
    templates_dir => "templates",
    static_dir => "static"
}.
```

Read at startup with `file:consult/1`. No special configuration framework.

---

## 8. Implementation Phases

### Phase 0: nera Prerequisites

These must land in the nera compiler/runtime before the web framework is
viable. They are useful independently and tracked in other design documents.

| Prerequisite                                | Status      | Document            | Why needed                       |
|---------------------------------------------|-------------|---------------------|----------------------------------|
| Binary strings (`-compile(binary_strings)`) | Designed    | STRING_MIGRATION.md | Sane HTTP string handling        |
| `file` module (read_file, write_file)       | Not started | STDLIB_DESIGN.md    | Templates, static files, config  |
| `json` module (encode/decode)               | Not started | STDLIB_DESIGN.md    | API responses                    |
| ETS (basic key-value)                       | Not started | RUNTIME_DESIGN.md   | Session storage, caches          |
| `gen_server` behaviour                      | Not started | STDLIB_DESIGN.md    | Stateful services                |
| `erlang:monotonic_time/1`                   | Not started | —                   | Logging, performance measurement |
| `iolist_to_binary/1`                        | Not started | —                   | Template output                  |
| `crypto:hash/2` or equivalent               | Not started | STDLIB_DESIGN.md    | Session tokens                   |
| `base64` module                             | Not started | STDLIB_DESIGN.md    | Cookie encoding                  |

### Phase 1: Minimal HTTP (weeks of work, not months)

**Goal:** Serve a "Hello, World" page from an Erlang source file.

Deliverables:
1. `nera_http` module — HTTP/1.1 request parser (binary pattern matching)
2. `nera_http_server` module — accept loop, process-per-connection
3. Response encoding (status line + headers + body)
4. Basic content types (text/html, text/plain, application/json)

**Depends on:** gen_tcp (already working), binary strings (Phase 0).

Test: `nera run hello_web.erl` → browser shows "Hello, World" at
`http://localhost:8080/`.

### Phase 2: Router + Middleware + Static Files

**Goal:** Multi-page app with URL routing, logging, and CSS.

Deliverables:
1. `nera_web:wrap/2` — middleware composition
2. `nera_web_logger` — request logging middleware
3. `nera_web_static` — static file serving middleware
4. Path splitting and pattern-match routing (documented patterns)
5. `nera_web` convenience functions (html, json, redirect, not_found)
6. `nera new` project scaffolding command

**Depends on:** Phase 1, `file` module (Phase 0).

Test: `nera new myapp && cd myapp && nera run` → working app with routes,
CSS, and logging.

### Phase 3: Templates

**Goal:** HTML templates with variable interpolation and control flow.

Deliverables:
1. Template parser (compiles `.html` files to Erlang modules)
2. Variable interpolation: `{{ var }}`
3. Control flow: `{% if %}`, `{% for %}`, `{% include %}`
4. Auto-escaping
5. Template inheritance: `{% extends "base.html" %}` / `{% block %}`
6. Integration with `nera_web:render/2`

**Depends on:** Phase 2, binary strings, `iolist_to_binary/1`.

Test: Render a page with a list of items from a map, verify HTML output.

### Phase 4: Data Layer

**Goal:** CRUD operations with a pleasant API.

Deliverables:
1. `nera_db` module — all, get, filter, insert, update, delete, count
2. ETS backend (in-memory, development)
3. Mnesia backend (persistent, production)
4. Auto-incrementing IDs
5. Map-based filter → match specification compiler

**Depends on:** ETS (Phase 0), Mnesia (later), `gen_server` (Phase 0).

Test: Create, read, update, delete records through the API. Verify persistence
with Mnesia backend.

### Phase 5: Sessions, Auth, Forms

**Goal:** Login/logout with server-side sessions.

Deliverables:
1. `nera_web_session` middleware — cookie-based session management
2. `nera_web_csrf` middleware — CSRF token generation and validation
3. `nera_form` module — form definition, validation, error reporting
4. Auth helpers — `require_auth` middleware, password hashing

**Depends on:** Phase 4, `crypto`/`base64` (Phase 0).

Test: Full login/signup flow with form validation and session persistence.

### Phase 6: Admin Generator

**Goal:** Auto-generated admin interface from model definitions.

Deliverables:
1. `nera_admin:register/3` — register a model with display options
2. Admin list view — paginated table with search
3. Admin detail view — edit form
4. Admin create/delete views
5. Built-in admin templates (clean, functional, not pretty)

**Depends on:** Phase 3 (templates), Phase 4 (data layer), Phase 5 (auth).

Test: Register two models, visit `/admin/`, verify CRUD operations work.

### Phase 7: Polish and Documentation

**Goal:** "Blog in 5 minutes" tutorial.

Deliverables:
1. Getting started tutorial (like Django's official tutorial)
2. API reference for all `nera_web` / `nera_http` / `nera_db` modules
3. Deployment guide (single binary advantage)
4. Error pages (404, 500) with debug info in dev mode
5. Hot code reload for development (nera's native advantage)

---

## 9. What Makes This Approach Credible

### Erlang's genuine advantages for web development

Most Erlang framework pitches focus on concurrency. That's necessary but not
sufficient. Here are the advantages that actually matter for web developers:

| Advantage                         | Why it matters                                                                                                                                                                                                                                 |
|-----------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **Single binary deployment**      | `nera build` produces one file. Copy it to the server. No virtualenv, no Docker, no dependency resolution on deploy. Django developers manage `requirements.txt`, `gunicorn`, `nginx`, `supervisor`, `collectstatic`. nera produces `./myapp`. |
| **Hot code reload**               | Change a handler, reload without dropping connections. Django developers restart the server. Phoenix developers restart the server. Erlang was doing zero-downtime deploys before Kubernetes existed.                                          |
| **Process-per-request isolation** | A bug in one request handler doesn't crash the server. No need for process managers (gunicorn workers) or exception middleware. If a handler process crashes, only that request fails.                                                         |
| **Natural WebSocket / real-time** | Every connection is already a process. Adding WebSocket support means keeping the process alive instead of closing it. No special library needed. Phoenix LiveView's advantage, but native to Erlang.                                          |
| **Soft real-time guarantees**     | Preemptive scheduling means one slow database query can't starve other requests. Django + Python's GIL can't guarantee this.                                                                                                                   |

### What we're NOT trying to do

- **Compete with Phoenix** for developers who already know Elixir.
- **Build a full ORM** — Erlang data access should be simpler, not SQL-shaped.
- **Support every HTTP feature** — HTTP/1.1 is sufficient to start. HTTP/2 is a
  later optimization.
- **Attract Ruby/Python developers** — the target audience is Erlang developers
  who need to build a web UI for their system, not web developers learning
  Erlang.

---

## 10. The Smallest Useful Thing

If we had to ship something tomorrow, what would be the minimal viable web
framework? Just three modules:

```erlang
%% 1. nera_http — parse and encode HTTP
%% 2. nera_web — convenience functions (render, redirect, json)
%% 3. nera_http_server — accept loop + process-per-connection
```

Plus the convention that your handler is a module exporting `handle/1` with
pattern-matching clauses.

That's enough to build real things. Everything else — templates, sessions,
admin — is additive, not foundational. Ship the core, iterate on the rest.

This is the Django lesson: **start with something that works end-to-end, then
add layers.** Don't design the perfect architecture. Build the smallest thing
that lets someone go from `nera new` to a working web page, then expand.
