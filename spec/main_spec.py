'''
tetron-wm main spec (packaging and catalog requirements, WMDEB-*, WMINST-*, WMCAT-*)
'''

from libspec import Spec
from . import packaging, catalog, constraints

class MainSpec(Spec):
    def modules(self):
        return [packaging, catalog, constraints]
