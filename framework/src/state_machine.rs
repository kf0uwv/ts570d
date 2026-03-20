//! State Machine Framework
//!
//! Provides application state management and state transitions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc};

/*
The StateMachine framework defines a set of states with handlers for new input from registered messages

 */

