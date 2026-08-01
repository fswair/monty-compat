class Item:
    pass

class Context:
    def __enter__(self):
        return 3

    def __exit__(self, exc_type, exc, tb):
        pass

item = Item()
with Context() as item.value:
    pass
item.value
