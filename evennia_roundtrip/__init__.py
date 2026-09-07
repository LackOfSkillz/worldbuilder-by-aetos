"""Read an Evennia game's areas, place them on a generated globe, and write the answer back.

Nothing in this package is imported by `worldbuilder/`, which is the conformance oracle and
does not move. The dependency runs one way: this reads the engine, the engine never reads
this.
"""
