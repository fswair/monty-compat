class Item:
    pass

class Context:
    def __enter__(self):
        return 3

    def __exit__(self, exc_type, exc, tb):
        pass

item = Item()
with Context() as _monty_compat_target_0:
    item.value = _monty_compat_target_0
    pass
item.value
