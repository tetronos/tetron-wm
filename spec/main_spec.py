'''
tetron-wm main spec (packaging and catalog requirements, WMDEB-*, WMINST-*, WMCAT-*, WMAST-*, WMNIX-*)
'''

from libspec import Spec
from . import packaging, catalog, constraints, assistant, nix

class MainSpec(Spec):
    def modules(self):
        return [packaging, catalog, constraints, assistant, nix]
