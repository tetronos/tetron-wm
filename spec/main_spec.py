'''
tetron-wm main spec (packaging and catalog requirements, WMDEB-*, WMINST-*, WMCAT-*, WMAST-*)
'''

from libspec import Spec
from . import packaging, catalog, constraints, assistant

class MainSpec(Spec):
    def modules(self):
        return [packaging, catalog, constraints, assistant]
