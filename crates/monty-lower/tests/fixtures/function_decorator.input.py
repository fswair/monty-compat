def decorate(fn):
    return lambda: fn() + 1

@decorate
def value():
    return 2

value()
