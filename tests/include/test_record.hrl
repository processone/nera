%% Record definition in a shared header file.
%% Used by include_record.erl to test that records from -include() work correctly.
-record(point, {x, y}).
-record(rect, {origin, width, height}).
