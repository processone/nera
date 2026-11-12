# Serverless Erlang with Nera

> **Status (2026-10-20): local demo only. Nera has no qualified AWS Lambda
> runtime.**
>
> - [`demo/serverless/`](../demo/serverless/) builds and runs a native
>   program on your computer. The program parses two sample packets compiled
>   into it and prints the results as Erlang terms. It reads no event.
> - Its `bootstrap` script is an untested sketch of a Lambda custom runtime.
>   Nobody has deployed it to AWS. Against a local fake Runtime API, it
>   posts the same non-JSON text for every event.
> - `nera build` produces an executable for the computer it runs on. Lambda
>   needs a Linux executable for the function's architecture (x86_64 or
>   arm64). A macOS build cannot run there, and complete Linux builds and
>   execution of Nera remain unverified (see [CONTRIBUTING.md](../CONTRIBUTING.md)).
> - No Lambda cold start, billed duration, memory setting or cost has been
>   measured. The numbers in this document are local process measurements.
>
> Do not cite this document as evidence of Lambda compatibility, concurrent
> request handling or lower cloud costs. The steps that would establish them
> are listed in
> [What a qualified Lambda function needs](#what-a-qualified-lambda-function-needs).

## Why serverless is worth exploring

Erlang's processes, message passing, links and supervisors suit event
handlers, and Nera compiles Erlang into one native executable that runs
without an Erlang installation. That makes short-lived functions an
interesting target. It is a goal, not a result: this document separates what
exists today from what still has to be built and measured.

---

## The local demo

### Files

```
demo/serverless/
├── handler.erl          # Parses two sample IPv4/TCP/UDP packets, prints Erlang terms
├── build.sh             # Builds handler.erl and runs it on this computer
├── bench_cold_start.sh  # Local process time and peak memory, Nera vs erl
└── bootstrap            # Untested sketch of a Lambda custom runtime loop
```

### Run it

```bash
# Build the compiler and the handler, then run the handler locally
./demo/serverless/build.sh

# Compare local process time and peak memory with erl (needs Erlang/OTP)
./demo/serverless/bench_cold_start.sh
```

`build.sh` builds the compiler with `cargo build --release --locked`;
`bench_cold_start.sh` expects that build. Both scripts can be run from any
folder. `build.sh --package` is described under [Packaging](#packaging).

The handler prints two Erlang terms (shortened here):

```
#{ip => #{version => 4,...,protocol => 6,...},transport => #{type => tcp,src_port => 49153,dst_port => 80,...}}
#{ip => #{version => 4,...,protocol => 17,...},transport => #{type => udp,src_port => 12345,dst_port => 53,length => 16}}
```

### What it demonstrates

The handler parses raw IP packet headers with bit-level binary pattern
matching:

```erlang
parse_ipv4(<<Version:4, IHL:4, _DSCP:6, _ECN:2, TotalLen:16,
             _Id:16, _Flags:3, _FragOff:13,
             TTL:8, Protocol:8, _Checksum:16,
             S1:8, S2:8, S3:8, S4:8,
             D1:8, D2:8, D3:8, D4:8,
             Rest/binary>>) ->
    ...
```

This Erlang code compiles into one native executable. On macOS arm64 it links
only to the system library (`libSystem`), not to an Erlang VM. The demo does
not read an event, does not produce JSON, and says nothing about Lambda.

### Local measurements

Measured on 2026-10-20 with `bench_cold_start.sh`, run twice, with the
compiler at main `8d575de`, on macOS 27 arm64 with Rust 1.99.0 and
Erlang/OTP 29 (ERTS 17.0.4). Each value is the mean of 10 runs:

| Program                                | Peak RSS, run 1 | Peak RSS, run 2 | Executable size                  |
|----------------------------------------|-----------------|-----------------|----------------------------------|
| Nera native executable                 | 5.1 MB          | 5.1 MB          | 3.3 MB (3,435,144 bytes)         |
| BEAM, `erl -noshell` with default boot | 72.4 MB         | 65.9 MB         | needs an Erlang/OTP installation |

Time from exec to exit is not compared here, because it varied too much with
machine load. The Nera executable took 126 ms in run 1 and 132 ms in run 2.
About 100 ms of that elapsed after the program had printed its output, while
the runtime shut down. `erl` took 133 ms in run 1 and 1,420 ms in run 2,
when the machine's load average was near 150.

How to read these numbers:

- They describe whole local processes, measured with `/usr/bin/time`. macOS
  reports wall time in 10 ms steps.
- The BEAM row starts `erl` with its default boot script. It is not an OTP
  release or a BEAM-based Lambda runtime.
- A Lambda cold start also includes creating the execution environment,
  loading the package and starting `bootstrap`. None of that was measured.
- Peak RSS is not what Lambda bills. Lambda bills the memory you configure,
  at least 128 MB, for the billed duration (see [Cost](#cost)).
- Only macOS arm64 was measured. Linux was not.

Earlier versions of this document reported a 26 ms cold start, 2.2 MB of
peak RSS and a 1.4 MB executable for this demo. Those figures could not be
reproduced on 2026-10-20 and are withdrawn.

---

## Lambda is not qualified

### What the bootstrap sketch does

[`demo/serverless/bootstrap`](../demo/serverless/bootstrap) is a POSIX shell
loop. For each invocation it:

1. calls `GET /2018-06-01/runtime/invocation/next` with `curl`, reads the
   request ID header and keeps the event body in a variable it never uses;
2. starts `./handler` as a new process;
3. posts the handler's output to `/invocation/{id}/response`, or a hand-built
   error message to `/invocation/{id}/error` if the handler fails.

On 2026-10-20 we ran it against a local fake Runtime API, with no AWS
service involved, and sent two different JSON events:

- both invocations returned the same two lines of Erlang terms, because the
  handler ignores the event;
- the responses were not JSON, and `curl` posted them with its default
  `Content-Type: application/x-www-form-urlencoded`;
- the handler's stdout and stderr are merged into the response;
- when `/next` answered with an HTTP error, the loop posted a response to
  `/invocation//response`, with an empty request ID.

It also does not report initialization errors, escape its error JSON,
propagate the trace header, honour the deadline or read `_HANDLER`. Whether
`curl` exists in the `provided.al2023` image was not checked.

### Packaging

`build.sh --package` writes `demo/serverless/build/lambda.zip` only when the
handler is a 64-bit Linux ELF executable. It prints the architecture it read
from that file, `x86_64` or `arm64`. On macOS it stops without a package,
because the handler is a Mach-O executable that Lambda cannot run. Earlier
versions of the script packaged the macOS executable and suggested deploying
it as `x86_64`.

Even on Linux, the package is unqualified: nobody has deployed it, its
library requirements were not checked against Amazon Linux 2023, and the
handler still ignores events.

For testing, `NERA_SERVERLESS_PREBUILT_HANDLER=PATH build.sh --package`
packages an existing executable instead of building and running
`handler.erl`. On 2026-10-20 we used it on macOS with real 64-bit Linux
executables for x86_64 and arm64. Each `lambda.zip` held `bootstrap` and the
unchanged executable, both with execute permission, and the script reported
the right architecture. The demo handler itself has not been built for
Linux, and nothing in these packages has been run on Linux.

`nera build --target` can emit Linux object files from macOS (see
`scripts/test-cross.sh`), but these scripts do not link a Linux executable
from macOS. The former `--docker` option of `build.sh` was never implemented
and has been removed.

### What AWS requires

From the AWS Lambda Developer Guide and pricing page, read on 2026-10-20:

- An OS-only runtime such as `provided.al2023` starts an executable file named
  `bootstrap`. Native binaries must be compiled for Linux and for the
  function's instruction set architecture, x86_64 or arm64.
  ([OS-only runtimes](https://docs.aws.amazon.com/lambda/latest/dg/runtimes-provided.html),
  [custom runtimes](https://docs.aws.amazon.com/lambda/latest/dg/runtimes-custom.html))
- A custom runtime reads `_HANDLER`, `LAMBDA_TASK_ROOT` and
  `AWS_LAMBDA_RUNTIME_API`, and reports initialization errors. It then loops:
  get the next event, propagate the tracing header, create a context, invoke
  the handler with the event and context, and post the response or the error.
  ([custom runtimes](https://docs.aws.amazon.com/lambda/latest/dg/runtimes-custom.html),
  [Runtime API](https://docs.aws.amazon.com/lambda/latest/dg/runtimes-api.html))
- Memory is configured from 128 MB to 10,240 MB in 1 MB increments; 128 MB is
  the default and the lowest setting. CPU is allocated in proportion to
  memory; at 1,769 MB a function has the equivalent of one vCPU.
  ([memory](https://docs.aws.amazon.com/lambda/latest/dg/configuration-memory.html))
- Duration cost depends on the memory allocated to the function, and
  initialization counts towards billed execution time.
  ([pricing](https://aws.amazon.com/lambda/pricing/),
  [custom runtimes](https://docs.aws.amazon.com/lambda/latest/dg/runtimes-custom.html))
- With the default capacity provider, the runtime processes one invocation
  at a time, and Lambda provisions a separate execution environment for each
  concurrent request.
  ([scaling](https://docs.aws.amazon.com/lambda/latest/dg/lambda-concurrency.html),
  [custom runtimes](https://docs.aws.amazon.com/lambda/latest/dg/runtimes-custom.html))
- Lambda Managed Instances can process several invocations in one execution
  environment. Its runtime must make concurrent `/next` calls up to
  `AWS_LAMBDA_MAX_CONCURRENCY`, handle concurrent responses and log in JSON.
  ([Managed Instances runtimes](https://docs.aws.amazon.com/lambda/latest/dg/runtimes-custom.html#runtimes-custom-managed-instances))

### Concurrency

Earlier versions of this document said that Nera could serve many concurrent
Lambda requests in one instance and so need fewer instances. That does not
hold for the default Lambda model: an execution environment handles one
invocation at a time, whatever the language. Erlang processes can split the
work of one invocation, but at small memory settings the function has only a
fraction of one vCPU.

Lambda Managed Instances is a different model. Nera has no runtime for it,
and it has not been tried.

Erlang process isolation contains a crash inside one operating-system
process. It is not a security boundary between users or tenants.

### Cost

Earlier versions of this document said that Nera functions would be two to
four times cheaper than BEAM and twice as cheap as Go, because of a small
local RSS. These claims are withdrawn:

- Lambda bills the memory you configure, and 128 MB is the minimum for every
  runtime, not a Nera advantage.
- No Nera function has run on Lambda, so its required memory setting, billed
  duration, initialization time and cold-start rate are unknown.

A cost comparison would have to measure, for the same workload, the memory
setting each runtime needs, billed duration including initialization,
cold-start rate and throughput, plus request, log and network charges.

---

## What a qualified Lambda function needs

1. **A local function with real input and output.** An executable that
   receives a real event, returns a JSON response that depends on it,
   reports errors as structured JSON and keeps logs out of the response,
   with deterministic local tests that need no cloud provider.
2. **Linux executables.** Build and run Nera programs on Linux x86_64 and
   arm64, and check their library requirements against Amazon Linux 2023.
3. **A Runtime API client.** A long-lived Nera process that implements the
   runtime loop above, one invocation at a time for the default model,
   tested against a local emulator such as the
   [Lambda Runtime Interface Emulator](https://github.com/aws/aws-lambda-runtime-interface-emulator).
4. **AWS qualification.** In a real account, with explicit approval before
   any spending: deploy for each architecture, invoke from a real trigger,
   and measure cold and warm starts, errors, timeouts and logs.
5. **Published measurements.** Only then publish Lambda timings or cost
   comparisons, with their protocol and raw data.

### Building blocks available today

| Need                                     | State in Nera (see [FEATURES.md](../FEATURES.md), [COMPATIBILITY.md](../COMPATIBILITY.md)) |
|------------------------------------------|---------------------------------------------------------------------------------------------|
| Read `AWS_LAMBDA_RUNTIME_API`            | `os:getenv/1` is available                                                                  |
| Read an event from a file                | `file:read_file/1` is available                                                             |
| Read an event from stdin                 | Not available: no `io:get_line/1`                                                           |
| HTTP/1.1 client for the Runtime API      | Not built in. `gen_tcp:connect/3` exists; a client would have to be written                 |
| JSON encode and decode                   | No built-in `json` module. The Thoas library has a scoped native pass                       |
| TLS/HTTPS client for other AWS services  | Not available                                                                               |

## Other platforms

Containers, Fly.io Machines, edge platforms and embedded Linux have not been
tried. Each would need an executable for its target that Nera does not yet
qualify, and WebAssembly targets would need a WebAssembly backend, which Nera
does not have. This document makes no claim about them.
