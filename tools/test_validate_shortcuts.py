import copy
import json
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
import validate_shortcuts as sc

ROOT=Path(__file__).resolve().parent.parent
def load(name):
    return json.loads((ROOT/'spec/vectors'/name).read_text(encoding='utf-8'))

class Shortcuts(unittest.TestCase):
    def test_accelerator_vectors(self):
        v=load('accelerators.json')
        for a,b in v['same']: self.assertEqual(sc.normalize(a),sc.normalize(b))
        for a,b in v['different']: self.assertNotEqual(sc.normalize(a),sc.normalize(b))
        for c in v['normalize']: self.assertEqual(sc.normalize(c['input']),c['expected'])
        for c in v['display']: self.assertEqual(sc.display(c['input'],c['os']),c['expected'])
        for c in v['conflicts']: self.assertEqual(sc.conflicts(c['a'],c['b']),c['expected'])
        for c in v['invalid']:
            with self.subTest(c=c),self.assertRaises(ValueError): sc.normalize(c)

    def test_shortcut_vectors(self):
        v=load('shortcuts.json')
        for c in v['valid']:
            doc=c.get('document',c.get('sheet'))
            with self.subTest(c=c['name']):
                sc.validate(doc,c.get('manifest'))
                self.assertEqual(sc.markdown(doc),c['markdown'])
        for c in v['invalid']:
            with self.subTest(c=c['name']),self.assertRaises(ValueError): sc.validate(c.get('document',c.get('sheet')),c.get('manifest'))

    def test_contexts_and_null_os_overrides(self):
        doc=copy.deepcopy(load('shortcuts.json')['valid'][0]['document'])
        doc['groups'][1]['shortcuts'][0]['keys']['default']='Ctrl+Alt+F'
        sc.validate(doc) # the same key in global and overlay is allowed
        self.assertEqual(sc.effective(doc['groups'][1]['shortcuts'][2],'windows'),[])
        self.assertIn('| Pin | Ctrl+Shift+P | Ctrl+Shift+P | ⇧⌘P |',sc.markdown(doc))

    def test_sheets_reserve_global_and_conflicting_alternatives(self):
        vectors=load('shortcuts.json')
        names={'third-party-global-is-reserved-for-system-sheets','duplicate-alternatives','prefix-alternatives','shadowed-default-alternatives'}
        for case in vectors['invalid']:
            if case['name'] in names:
                with self.subTest(name=case['name']),self.assertRaises(ValueError):
                    sc.validate(case.get('sheet',case.get('document')))
        for case in vectors['valid']:
            if case['name'] in {'windows-system-global-context','macos-system-global-context','microphone-and-horizontal-scroll-sheet'}:
                sc.validate(case['sheet'])

    def test_cli(self):
        with tempfile.TemporaryDirectory() as d:
            file=Path(d)/'shortcuts.json';file.write_text(json.dumps(load('shortcuts.json')['valid'][0]['document']))
            result=subprocess.run([sys.executable,str(ROOT/'tools/validate_shortcuts.py'),str(file),'--markdown'],capture_output=True,text=True,encoding='utf-8')
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertIn('| Action | Linux | Windows | macOS |',result.stdout)
            self.assertIn('⇧⌘P',result.stdout)
            file.write_text('{}')
            self.assertEqual(subprocess.run([sys.executable,str(ROOT/'tools/validate_shortcuts.py'),str(file)],capture_output=True).returncode,1)

    def test_schema_structure_and_valid_vectors(self):
        try: import jsonschema
        except ImportError: self.skipTest('jsonschema not installed (stdlib validator is tested)')
        for name in ['shortcuts.schema.json','shortcut-sheet.schema.json','receipt.schema.json']:
            schema=json.loads((ROOT/'spec'/name).read_text())
            jsonschema.Draft202012Validator.check_schema(schema)
        for c in load('shortcuts.json')['valid']:
            sheet='sheet' in c
            schema=json.loads((ROOT/'spec'/('shortcut-sheet.schema.json' if sheet else 'shortcuts.schema.json')).read_text())
            jsonschema.validate(c['sheet' if sheet else 'document'],schema)

    def test_schema_key_patterns_match_canonical_notation(self):
        def patterns(value):
            if isinstance(value, dict):
                if 'pattern' in value and 'Numpad' in value['pattern']: yield value['pattern']
                for v in value.values(): yield from patterns(v)
            elif isinstance(value, list):
                for v in value: yield from patterns(v)
        vectors=load('accelerators.json')
        for name in ['shortcuts.schema.json','shortcut-sheet.schema.json']:
            rules=list(patterns(json.loads((ROOT/'spec'/name).read_text())))
            self.assertTrue(rules)
            for rule in rules:
                for v in vectors['normalize']:
                    self.assertIsNotNone(re.fullmatch(rule,v['expected']),v)
                for v in vectors['invalid']:
                    self.assertIsNone(re.fullmatch(rule,v),v)

if __name__=='__main__': unittest.main()
