def decorate(fn):
    return lambda: fn() + 1

_monty_compat_decorator_0 = decorate
def value():
    return 2
value = _monty_compat_decorator_0(value)

value()
