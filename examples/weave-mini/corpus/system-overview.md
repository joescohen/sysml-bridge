# Cargo Handling System — Overview

This document describes the components, functions, and modes of the cargo handling subsystem installed on the flight line.

## Components

The Cargo Handling Controller coordinates all cargo handling activity from a central rack-mounted unit.

The Position Sensor Array detects pallet position along the conveyor path.

The Conveyor Drive Motor drives the main conveyor belt at variable speed.

The Load Cell Assembly measures the weight of each pallet as it crosses the load station.

## Functions

The Detect Cargo Presence function signals the controller when a pallet enters the load zone.

The Compute Load Distribution function calculates the weight distribution across the conveyor deck.

The Monitor Conveyor Speed function tracks belt speed against the commanded setpoint.

## Modes

The system enters the Interlock mode whenever an obstruction is detected on the conveyor path.

## Requirements

REQ-001: The system shall detect pallet presence within 200 milliseconds of entry into the load zone.

REQ-002: The system shall compute load distribution across the conveyor deck within 1 second of a weight reading.

REQ-003: The system shall enter the Interlock mode within 100 milliseconds of an obstruction detection.
