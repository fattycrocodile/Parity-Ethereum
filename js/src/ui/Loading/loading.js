// Copyright 2015-2017 Parity Technologies (UK) Ltd.
// This file is part of Parity.

// Parity is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity.  If not, see <http://www.gnu.org/licenses/>.

import React, { PropTypes } from 'react';
import CircularProgress from 'material-ui/CircularProgress';

import styles from './loading.css';

export default function Loading ({ className, size, thickness }) {
  return (
    <div className={ [ styles.loading, className ].join(' ') }>
      <CircularProgress
        size={ size * 60 }
        thickness={ thickness }
      />
    </div>
  );
}

Loading.propTypes = {
  className: PropTypes.string,
  size: PropTypes.number,
  thickness: PropTypes.number
};

Loading.defaultProps = {
  className: '',
  size: 2,
  thickness: 3.5
};
