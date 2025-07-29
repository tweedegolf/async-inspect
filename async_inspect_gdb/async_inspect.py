import async_inspect_gdb
import gdb

gdb.register_window_type("async_inspect_window", async_inspect_gdb.GdbTui)
gdb.execute("tui new-layout async_inspect async_inspect_window 1 status 0 cmd 1")
print("Async inspect loaded")
