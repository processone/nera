# Run upstream assertions unchanged. Only the public Thoas modules are replaced
# by transport proxies in native mode; ExUnit, EUnit and generators stay on BEAM.
defmodule NativeTransport do
  def call(module, function, args) do
    dir = Path.join(System.fetch_env!("THOAS_CALLS"), Integer.to_string(System.unique_integer([:positive])))
    File.mkdir_p!(dir)
    File.write!(Path.join(dir, "request.etf"), :erlang.term_to_binary({module, function, args}))
    {output, status} = System.cmd(System.fetch_env!("THOAS_PYTHON"),
      [System.fetch_env!("THOAS_RUNNER"), "--bridge", System.fetch_env!("THOAS_NATIVE"), dir], stderr_to_stdout: true)
    if status != 0, do: raise("Native transport failed in #{dir}: #{output}")
    case :erlang.binary_to_term(File.read!(Path.join(dir, "reply.etf"))) do
      {:return, value} -> value
      {:exception, :error, reason} -> :erlang.error(reason)
      {:exception, :throw, reason} -> throw(reason)
      {:exception, :exit, reason} -> exit(reason)
    end
  end
end

defmodule UpstreamResults do
  use GenServer
  def init(_), do: {:ok, nil}
  def handle_cast({:test_finished, test}, state) do
    File.write!(System.fetch_env!("THOAS_TEST_RESULTS"),
      "#{inspect(test.module)}\t#{test.name}\t#{inspect(test.state, limit: :infinity)}\n", [:append])
    {:noreply, state}
  end
  def handle_cast(_, state), do: {:noreply, state}
end

Code.prepend_path(System.fetch_env!("THOAS_BEAM"))
Code.prepend_path(System.fetch_env!("THOAS_EUNIT"))

if System.fetch_env!("THOAS_ENGINE") == "nera" do
  for module <- [:thoas, :thoas_encode] do
    exports = module.module_info(:exports) |> Enum.reject(fn {name, _} -> name == :module_info end)
    definitions = for {name, arity} <- exports do
      args = Macro.generate_arguments(arity, __MODULE__)
      quote do
        def unquote(name)(unquote_splicing(args)),
          do: NativeTransport.call(unquote(module), unquote(name), [unquote_splicing(args)])
      end
    end
    :code.purge(module)
    :code.delete(module)
    Module.create(module, definitions, Macro.Env.location(__ENV__))
  end
end

# Compiled from the untouched upstream Erlang test, including its real EUnit header.
true = Code.ensure_loaded?(:thoas_encode_test)
eunit = :eunit.test(:thoas_encode_test, [:verbose, {:timeout, 120}])
IO.puts("UPSTREAM_EUNIT:#{eunit}")

# Refuse the upstream conditional that would silently omit all property tests.
true = Code.ensure_loaded?(ExUnitProperties)
Application.put_env(:stream_data, :max_runs, 100)
ExUnit.start(autorun: false, seed: 0, max_cases: 1, timeout: 300_000,
  formatters: [ExUnit.CLIFormatter, UpstreamResults])
for path <- Path.wildcard("test/*_test.exs"), do: Code.require_file(path)
summary = ExUnit.run()
IO.puts("UPSTREAM_EXUNIT:#{summary.total}:#{summary.failures}:#{summary.excluded}:#{summary.skipped}")
System.halt(if eunit == :ok and summary.failures == 0, do: 0, else: 1)
