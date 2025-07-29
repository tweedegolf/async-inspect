# Setup
- Install [`uv`](https://github.com/astral-sh/uv)
- Build project: `uv run maturin develop`

- Add the following to `~/.config/gdb/gdbinit` to make gdb work with python virtual environments
```
python
# Update GDB's Python paths with the `sys.path` values of the local
#  Python installation, whether that is brew'ed Python, a virtualenv,
#  or another system python.

# Convert GDB to interpret in Python
import os,subprocess,sys
# Execute a Python using the user's shell and pull out the sys.path (for site-packages)
paths = subprocess.check_output('python -c "import os,sys;print(os.linesep.join(sys.path).strip())"',shell=True).decode("utf-8").split()
# Extend GDB's Python's search path
sys.path.extend(paths)
end
```
- Enter the virtual enviroment
```
source ./.venv/bin/activate
```
- Start GDB

- Run the following commands
```
source async_inspect.py
tui layout async_inspect
```
