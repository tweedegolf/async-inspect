import gdb_backend
import gdb

# It is not posible to extend python class in py03 yet, it is alos not posible te skip a breakpoint
# without a deriving a custom implementation. So creating the implementation here in python
class PyO3Breakpoint(gdb.Breakpoint):
    # Set the callback to call when the breakpoint is hit. It will be called with as first argument
    # the second argument given to this function.
    # 
    # The callback is not allowed the change the gdb state, only read values.
    # It can return `True` to stop at the hit breakpoint, any other value will make gdb continue
    # executing.
    def set_stop_callback(self, stop_callback, callback_data = None):
        self.stop_callback = stop_callback
        self.callback_data = callback_data

    def stop(self):
        if hasattr(self, "stop_callback"):
            return self.stop_callback(self.callback_data)
        else:
            # Always break on breakpoints while the callback is not set to retain the default behavior
            return True


gdb.register_window_type("inspect_embassy_window", gdb_backend.GdbTui)
gdb.execute("tui new-layout inspect_embassy inspect_embassy_window 1 status 0 cmd 1")
print("inspect-embassy loaded")

